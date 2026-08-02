use llm::{ChatMessage, CompletionRequest, LlmProvider};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Node {
    pub id: Uuid,
    pub node_type: String,
    pub config: Value,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: Uuid,
    pub target: Uuid,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct NodeResult {
    pub node_id: Uuid,
    pub status: NodeStatus,
    pub output: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    Succeeded,
    Failed,
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub status: ExecutionStatus,
    pub nodes: Vec<NodeResult>,
}

/// Channel a caller can pass to `execute` to observe each `NodeResult` as it's produced,
/// for live-trace UIs. Send failures (no receiver left) are ignored.
pub type EventSender = tokio::sync::mpsc::UnboundedSender<NodeResult>;

fn emit(events: Option<&EventSender>, result: &NodeResult) {
    if let Some(tx) = events {
        let _ = tx.send(result.clone());
    }
}

/// Runs a workflow graph to completion in-process (no queue/workers yet).
///
/// Nodes are processed in topological order. A node whose inbound edges never fire (its
/// upstream failed, or a condition edge didn't match) is marked `Skipped` rather than run,
/// and that skip propagates downstream unless another path also feeds the same node.
///
/// `provider` is used by `agent` nodes to call an LLM; pass `None` if no provider is
/// configured (agent nodes will then fail with a clear error, everything else is unaffected).
pub async fn execute(
    nodes: &[Node],
    edges: &[Edge],
    provider: Option<&dyn LlmProvider>,
    events: Option<&EventSender>,
) -> ExecutionResult {
    let node_map: HashMap<Uuid, &Node> = nodes.iter().map(|n| (n.id, n)).collect();

    let mut inbound: HashMap<Uuid, Vec<&Edge>> = HashMap::new();
    let mut outbound: HashMap<Uuid, Vec<&Edge>> = HashMap::new();
    for e in edges {
        inbound.entry(e.target).or_default().push(e);
        outbound.entry(e.source).or_default().push(e);
    }

    let mut unresolved: HashMap<Uuid, usize> = nodes
        .iter()
        .map(|n| (n.id, inbound.get(&n.id).map_or(0, Vec::len)))
        .collect();

    let mut fired_inbound: HashMap<Uuid, Vec<Value>> = HashMap::new();
    let mut results: Vec<NodeResult> = Vec::new();
    let mut queue: VecDeque<Uuid> = VecDeque::new();
    let mut visited: HashSet<Uuid> = HashSet::new();

    for n in nodes {
        if unresolved.get(&n.id).copied().unwrap_or(0) == 0 {
            queue.push_back(n.id);
        }
    }

    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id) {
            continue;
        }

        let inputs = fired_inbound.remove(&node_id).unwrap_or_default();
        let had_inbound = inbound.get(&node_id).is_some_and(|v| !v.is_empty());

        if had_inbound && inputs.is_empty() {
            let result = NodeResult {
                node_id,
                status: NodeStatus::Skipped,
                output: None,
                error: None,
            };
            emit(events, &result);
            results.push(result);
            propagate(
                node_id,
                None,
                &outbound,
                &mut unresolved,
                &mut fired_inbound,
                &mut queue,
            );
            continue;
        }

        let Some(node) = node_map.get(&node_id) else {
            continue;
        };

        match run_node(node, &inputs, provider).await {
            Ok(output) => {
                propagate(
                    node_id,
                    Some(&output),
                    &outbound,
                    &mut unresolved,
                    &mut fired_inbound,
                    &mut queue,
                );
                let result = NodeResult {
                    node_id,
                    status: NodeStatus::Succeeded,
                    output: Some(output),
                    error: None,
                };
                emit(events, &result);
                results.push(result);
            }
            Err(err) => {
                propagate(
                    node_id,
                    None,
                    &outbound,
                    &mut unresolved,
                    &mut fired_inbound,
                    &mut queue,
                );
                let result = NodeResult {
                    node_id,
                    status: NodeStatus::Failed,
                    output: None,
                    error: Some(err),
                };
                emit(events, &result);
                results.push(result);
            }
        }
    }

    for n in nodes {
        if !visited.contains(&n.id) {
            let result = NodeResult {
                node_id: n.id,
                status: NodeStatus::Failed,
                output: None,
                error: Some("node was never resolved (cycle in workflow graph?)".into()),
            };
            emit(events, &result);
            results.push(result);
        }
    }

    let status = if results.iter().any(|r| r.status == NodeStatus::Failed) {
        ExecutionStatus::Failed
    } else {
        ExecutionStatus::Succeeded
    };

    ExecutionResult {
        status,
        nodes: results,
    }
}

/// Marks each outgoing edge of `source_id` as fired or not, decrementing the target's
/// unresolved-inbound-edge count and enqueueing it once every inbound edge is accounted for.
fn propagate(
    source_id: Uuid,
    output: Option<&Value>,
    outbound: &HashMap<Uuid, Vec<&Edge>>,
    unresolved: &mut HashMap<Uuid, usize>,
    fired_inbound: &mut HashMap<Uuid, Vec<Value>>,
    queue: &mut VecDeque<Uuid>,
) {
    let Some(out_edges) = outbound.get(&source_id) else {
        return;
    };

    for edge in out_edges {
        let fires = match (&edge.condition, output) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(cond), Some(out)) => {
                out.get("result")
                    .and_then(Value::as_bool)
                    .map(|b| b.to_string())
                    .as_deref()
                    == Some(cond.as_str())
            }
        };

        if let Some(out) = output.filter(|_| fires) {
            fired_inbound
                .entry(edge.target)
                .or_default()
                .push(out.clone());
        }

        if let Some(count) = unresolved.get_mut(&edge.target) {
            *count -= 1;
            if *count == 0 {
                queue.push_back(edge.target);
            }
        }
    }
}

fn merge_inputs(inputs: &[Value]) -> Value {
    match inputs.len() {
        0 => Value::Object(Default::default()),
        1 => inputs[0].clone(),
        _ => serde_json::json!({ "inputs": inputs }),
    }
}

/// Replaces `{{field}}` placeholders in `template` with top-level values from `context`.
/// String values are substituted verbatim; other JSON values are substituted as JSON.
fn render_template(template: &str, context: &Value) -> String {
    let Some(object) = context.as_object() else {
        return template.to_string();
    };

    let mut rendered = template.to_string();
    for (key, value) in object {
        let placeholder = format!("{{{{{key}}}}}");
        let replacement = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        rendered = rendered.replace(&placeholder, &replacement);
    }
    rendered
}

async fn run_node(
    node: &Node,
    inputs: &[Value],
    provider: Option<&dyn LlmProvider>,
) -> Result<Value, String> {
    match node.node_type.as_str() {
        "input" => Ok(node.config.get("value").cloned().unwrap_or(Value::Null)),

        "transform" => {
            let base = merge_inputs(inputs);
            let mut object = match base {
                Value::Object(map) => map,
                Value::Null => serde_json::Map::new(),
                other => {
                    let mut map = serde_json::Map::new();
                    map.insert("input".to_string(), other);
                    map
                }
            };
            if let Some(merge) = node.config.get("merge").and_then(Value::as_object) {
                for (k, v) in merge {
                    object.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(object))
        }

        "condition" => {
            let base = merge_inputs(inputs);
            let field = node
                .config
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| "condition node missing \"field\" in config".to_string())?;
            let expected = node.config.get("equals").cloned().unwrap_or(Value::Null);
            let actual = base.get(field).cloned().unwrap_or(Value::Null);
            Ok(serde_json::json!({ "result": actual == expected }))
        }

        "agent" => {
            let provider = provider
                .ok_or_else(|| "no LLM provider configured (set LLM_PROVIDER)".to_string())?;

            let prompt_template = node
                .config
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or_else(|| "agent node missing \"prompt\" in config".to_string())?;
            let model = node
                .config
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| "agent node missing \"model\" in config".to_string())?;
            let system = node
                .config
                .get("system")
                .and_then(Value::as_str)
                .map(str::to_string);
            let max_tokens = node
                .config
                .get("max_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(1024) as u32;
            // Tool list support (config["tools"]) lands with MCP client support later in Phase 2;
            // the field is accepted here so node configs can already carry it.

            let context = merge_inputs(inputs);
            let prompt = render_template(prompt_template, &context);

            let request = CompletionRequest {
                model: model.to_string(),
                system,
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: prompt,
                }],
                max_tokens,
            };

            let response = provider
                .complete(&request)
                .await
                .map_err(|e| format!("agent node LLM call failed: {e}"))?;

            Ok(serde_json::json!({ "response": response }))
        }

        other => Err(format!("unknown node type: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: Uuid, node_type: &str, config: Value) -> Node {
        Node {
            id,
            node_type: node_type.to_string(),
            config,
        }
    }

    fn edge(source: Uuid, target: Uuid, condition: Option<&str>) -> Edge {
        Edge {
            source,
            target,
            condition: condition.map(str::to_string),
        }
    }

    struct EchoProvider;

    #[async_trait::async_trait]
    impl LlmProvider for EchoProvider {
        async fn complete(&self, request: &CompletionRequest) -> Result<String, llm::LlmError> {
            Ok(request.messages[0].content.clone())
        }

        fn name(&self) -> &'static str {
            "echo"
        }
    }

    fn status_of(result: &ExecutionResult, id: Uuid) -> NodeStatus {
        result
            .nodes
            .iter()
            .find(|n| n.node_id == id)
            .unwrap()
            .status
    }

    fn output_of(result: &ExecutionResult, id: Uuid) -> &Value {
        result
            .nodes
            .iter()
            .find(|n| n.node_id == id)
            .unwrap()
            .output
            .as_ref()
            .unwrap()
    }

    #[tokio::test]
    async fn linear_chain_propagates_and_merges() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        let nodes = vec![
            node(a, "input", serde_json::json!({ "value": { "x": 1 } })),
            node(b, "transform", serde_json::json!({ "merge": { "y": 2 } })),
            node(c, "transform", serde_json::json!({ "merge": { "z": 3 } })),
        ];
        let edges = vec![edge(a, b, None), edge(b, c, None)];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(
            *output_of(&result, c),
            serde_json::json!({ "x": 1, "y": 2, "z": 3 })
        );
    }

    #[tokio::test]
    async fn condition_true_branch_runs_and_false_branch_is_skipped() {
        let input = Uuid::new_v4();
        let cond = Uuid::new_v4();
        let on_true = Uuid::new_v4();
        let on_false = Uuid::new_v4();

        let nodes = vec![
            node(
                input,
                "input",
                serde_json::json!({ "value": { "flag": true } }),
            ),
            node(
                cond,
                "condition",
                serde_json::json!({ "field": "flag", "equals": true }),
            ),
            node(
                on_true,
                "transform",
                serde_json::json!({ "merge": { "branch": "true" } }),
            ),
            node(
                on_false,
                "transform",
                serde_json::json!({ "merge": { "branch": "false" } }),
            ),
        ];
        let edges = vec![
            edge(input, cond, None),
            edge(cond, on_true, Some("true")),
            edge(cond, on_false, Some("false")),
        ];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(status_of(&result, on_true), NodeStatus::Succeeded);
        assert_eq!(status_of(&result, on_false), NodeStatus::Skipped);
    }

    #[tokio::test]
    async fn condition_false_branch_runs_and_true_branch_is_skipped() {
        let input = Uuid::new_v4();
        let cond = Uuid::new_v4();
        let on_true = Uuid::new_v4();
        let on_false = Uuid::new_v4();

        let nodes = vec![
            node(
                input,
                "input",
                serde_json::json!({ "value": { "flag": false } }),
            ),
            node(
                cond,
                "condition",
                serde_json::json!({ "field": "flag", "equals": true }),
            ),
            node(on_true, "transform", Value::Null),
            node(on_false, "transform", Value::Null),
        ];
        let edges = vec![
            edge(input, cond, None),
            edge(cond, on_true, Some("true")),
            edge(cond, on_false, Some("false")),
        ];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(status_of(&result, on_true), NodeStatus::Skipped);
        assert_eq!(status_of(&result, on_false), NodeStatus::Succeeded);
    }

    #[tokio::test]
    async fn condition_missing_field_defaults_to_false() {
        let input = Uuid::new_v4();
        let cond = Uuid::new_v4();

        let nodes = vec![
            node(input, "input", serde_json::json!({ "value": {} })),
            node(
                cond,
                "condition",
                serde_json::json!({ "field": "missing", "equals": true }),
            ),
        ];
        let edges = vec![edge(input, cond, None)];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(
            *output_of(&result, cond),
            serde_json::json!({ "result": false })
        );
    }

    #[tokio::test]
    async fn fan_in_collects_all_inputs() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let join = Uuid::new_v4();

        let nodes = vec![
            node(a, "input", serde_json::json!({ "value": "from-a" })),
            node(b, "input", serde_json::json!({ "value": "from-b" })),
            node(join, "transform", Value::Null),
        ];
        let edges = vec![edge(a, join, None), edge(b, join, None)];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        let output = output_of(&result, join);
        let inputs = output.get("inputs").and_then(Value::as_array).unwrap();
        assert_eq!(inputs.len(), 2);
    }

    #[tokio::test]
    async fn unknown_node_type_fails() {
        let a = Uuid::new_v4();
        let nodes = vec![node(a, "bogus", Value::Null)];
        let result = execute(&nodes, &[], None, None).await;

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
    }

    #[tokio::test]
    async fn upstream_failure_skips_downstream_but_marks_execution_failed() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let nodes = vec![
            node(a, "bogus", Value::Null),
            node(b, "transform", Value::Null),
        ];
        let edges = vec![edge(a, b, None)];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
        assert_eq!(status_of(&result, b), NodeStatus::Skipped);
    }

    #[tokio::test]
    async fn cycle_leaves_nodes_unresolved_and_fails() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let nodes = vec![
            node(a, "transform", Value::Null),
            node(b, "transform", Value::Null),
        ];
        let edges = vec![edge(a, b, None), edge(b, a, None)];

        let result = execute(&nodes, &edges, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
        assert_eq!(status_of(&result, b), NodeStatus::Failed);
    }

    #[tokio::test]
    async fn agent_node_renders_template_and_calls_provider() {
        let input = Uuid::new_v4();
        let agent = Uuid::new_v4();

        let nodes = vec![
            node(
                input,
                "input",
                serde_json::json!({ "value": { "name": "Sanket" } }),
            ),
            node(
                agent,
                "agent",
                serde_json::json!({ "prompt": "hello {{name}}", "model": "test-model" }),
            ),
        ];
        let edges = vec![edge(input, agent, None)];

        let result = execute(&nodes, &edges, Some(&EchoProvider), None).await;

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(
            *output_of(&result, agent),
            serde_json::json!({ "response": "hello Sanket" })
        );
    }

    #[tokio::test]
    async fn agent_node_without_provider_fails() {
        let a = Uuid::new_v4();
        let nodes = vec![node(
            a,
            "agent",
            serde_json::json!({ "prompt": "hi", "model": "test-model" }),
        )];

        let result = execute(&nodes, &[], None, None).await;

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
    }
}

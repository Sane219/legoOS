use llm::{ChatMessage, CompletionRequest, EmbeddingProvider, LlmProvider};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

/// What a `rag` node needs to search a workspace's knowledge base — bundled since none of
/// it is meaningful on its own (a client with no embedding provider, or vice versa, can't
/// actually run a search).
pub struct RagContext<'a> {
    pub client: &'a rag::RagClient,
    pub embedding_provider: &'a dyn EmbeddingProvider,
    pub workspace_id: Uuid,
}

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
    /// An `approval` node with no decision yet. Its outgoing edges don't fire — the
    /// nodes downstream of it simply stay unresolved until a resumed run supplies a
    /// decision (see `ResumeState`), rather than being marked Failed as "never resolved".
    Waiting,
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
    /// At least one `approval` node is blocked on a decision. Takes priority over
    /// `Failed` so an operator sees "needs your input" rather than "failed".
    Waiting,
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub status: ExecutionStatus,
    pub nodes: Vec<NodeResult>,
}

/// Lets a caller resume a previously paused run (one with an `approval` node still
/// `Waiting`) without recomputing nodes that already ran — critical for nodes with side
/// effects (an `agent` node's LLM call, an MCP tool call): replaying them would repeat
/// those effects rather than just restating their recorded output.
#[derive(Debug, Default)]
pub struct ResumeState {
    /// Prior results for nodes that already completed (anything but `Waiting` — a
    /// `Waiting` result is never seeded; the gate it belongs to is re-evaluated fresh
    /// against `approval_decisions` below instead of being replayed).
    pub seed_results: Vec<NodeResult>,
    /// Decisions for `approval` nodes being resumed right now: `true` = approved (the
    /// node succeeds and its edges fire), `false` = rejected (fails, no propagation).
    /// An approval node with no entry here (and no seed) is encountered fresh and
    /// produces `Waiting`, same as on a first run.
    pub approval_decisions: HashMap<Uuid, bool>,
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
    resume: Option<&ResumeState>,
    rag: Option<&RagContext<'_>>,
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

    // Defensive against a caller passing a `Waiting` entry through `seed_results` (it's
    // documented not to, but replaying "waiting" would just re-pause forever): a gate
    // is always re-evaluated fresh against `approval_decisions`, never replayed as-is.
    let seed_by_node: HashMap<Uuid, &NodeResult> = resume
        .map(|r| {
            r.seed_results
                .iter()
                .filter(|res| res.status != NodeStatus::Waiting)
                .map(|res| (res.node_id, res))
                .collect()
        })
        .unwrap_or_default();
    let empty_decisions = HashMap::new();
    let approval_decisions = resume.map_or(&empty_decisions, |r| &r.approval_decisions);

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

        // Replay a node that already completed in a prior (paused) run instead of
        // recomputing it — recomputing would repeat any side effect (an LLM call, an
        // MCP tool call) the node performed the first time.
        if let Some(seed) = seed_by_node.get(&node_id) {
            let result = (*seed).clone();
            emit(events, &result);
            let propagate_output = matches!(result.status, NodeStatus::Succeeded)
                .then(|| result.output.clone())
                .flatten();
            propagate(
                node_id,
                propagate_output.as_ref(),
                &outbound,
                &mut unresolved,
                &mut fired_inbound,
                &mut queue,
            );
            results.push(result);
            continue;
        }

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

        if node.node_type == "approval" {
            let result = match approval_decisions.get(&node_id) {
                Some(true) => NodeResult {
                    node_id,
                    status: NodeStatus::Succeeded,
                    output: Some(merge_inputs(&inputs)),
                    error: None,
                },
                Some(false) => NodeResult {
                    node_id,
                    status: NodeStatus::Failed,
                    output: None,
                    error: Some("approval rejected".to_string()),
                },
                None => NodeResult {
                    node_id,
                    status: NodeStatus::Waiting,
                    output: Some(merge_inputs(&inputs)),
                    error: None,
                },
            };
            emit(events, &result);
            if result.status == NodeStatus::Succeeded {
                propagate(
                    node_id,
                    result.output.as_ref(),
                    &outbound,
                    &mut unresolved,
                    &mut fired_inbound,
                    &mut queue,
                );
            } else if result.status == NodeStatus::Failed {
                propagate(
                    node_id,
                    None,
                    &outbound,
                    &mut unresolved,
                    &mut fired_inbound,
                    &mut queue,
                );
            }
            // Waiting: no propagation — downstream nodes intentionally stay unresolved
            // until this gate is decided in a resumed run.
            results.push(result);
            continue;
        }

        match run_node(node, &inputs, provider, rag).await {
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

    let any_waiting = results.iter().any(|r| r.status == NodeStatus::Waiting);

    // A node left unvisited is normally a cycle. But a node downstream of a `Waiting`
    // approval gate is *supposed* to stay unvisited until the gate is decided — reporting
    // it as Failed would be wrong, so once any gate is waiting we just leave those nodes
    // out of `results` entirely (they aren't cycles, we simply can't tell which unvisited
    // nodes are "blocked behind the gate" from "genuinely cyclic" without a full reachability
    // pass, and blocked-by-a-pending-approval is by far the common case here).
    if !any_waiting {
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
    }

    let status = if any_waiting {
        ExecutionStatus::Waiting
    } else if results.iter().any(|r| r.status == NodeStatus::Failed) {
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

/// Runs each `{ "mcp_url", "mcp_token"?, "tool", "arguments"? }` spec against its MCP
/// server and inserts the result into `context` under the tool's name, so the agent's
/// prompt template can pick it up via `{{tool_name}}` — same mechanism as any other field.
async fn run_tool_calls(tool_specs: &[Value], context: &mut Value) -> Result<(), String> {
    if tool_specs.is_empty() {
        return Ok(());
    }
    if !context.is_object() {
        let previous = std::mem::replace(context, Value::Object(Default::default()));
        context["input"] = previous;
    }

    for spec in tool_specs {
        let url = spec
            .get("mcp_url")
            .and_then(Value::as_str)
            .ok_or_else(|| "tool spec missing \"mcp_url\"".to_string())?;
        let tool_name = spec
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| "tool spec missing \"tool\"".to_string())?;
        let token = spec.get("mcp_token").and_then(Value::as_str);
        let arguments = spec
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));

        let client = mcp::McpClient::connect(url, token)
            .await
            .map_err(|e| format!("MCP connect to {url} failed: {e}"))?;
        let result = client
            .call_tool(tool_name, arguments)
            .await
            .map_err(|e| format!("MCP tool call {tool_name} failed: {e}"))?;
        let _ = client.close().await;

        context[tool_name] = result;
    }

    Ok(())
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
    rag: Option<&RagContext<'_>>,
) -> Result<Value, String> {
    match node.node_type.as_str() {
        "input" => Ok(node.config.get("value").cloned().unwrap_or(Value::Null)),

        "rag" => {
            let rag = rag.ok_or_else(|| {
                "no RAG context configured (Qdrant/embedding provider unavailable)".to_string()
            })?;

            let query_template = node
                .config
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "rag node missing \"query\" in config".to_string())?;
            let limit = node
                .config
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(5);

            let context = merge_inputs(inputs);
            let query = render_template(query_template, &context);

            let vector = rag
                .embedding_provider
                .embed(&query)
                .await
                .map_err(|e| format!("rag node embedding failed: {e}"))?;
            let hits = rag
                .client
                .search(rag.workspace_id, vector, limit)
                .await
                .map_err(|e| format!("rag node search failed: {e}"))?;

            let chunks: Vec<Value> = hits
                .into_iter()
                .map(|hit| {
                    serde_json::json!({
                        "text": hit.text,
                        "document_id": hit.document_id,
                        "chunk_index": hit.chunk_index,
                        "score": hit.score,
                    })
                })
                .collect();

            Ok(serde_json::json!({ "chunks": chunks }))
        }

        "memory_write" => {
            let rag = rag.ok_or_else(|| {
                "no RAG context configured (Qdrant/embedding provider unavailable)".to_string()
            })?;

            let agent_key = node
                .config
                .get("agent_key")
                .and_then(Value::as_str)
                .ok_or_else(|| "memory_write node missing \"agent_key\" in config".to_string())?;
            let content_template = node
                .config
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| "memory_write node missing \"content\" in config".to_string())?;

            let context = merge_inputs(inputs);
            let content = render_template(content_template, &context);

            let vector = rag
                .embedding_provider
                .embed(&content)
                .await
                .map_err(|e| format!("memory_write node embedding failed: {e}"))?;

            rag.client
                .ensure_memories_collection(vector.len() as u64)
                .await
                .map_err(|e| format!("memory_write node failed: {e}"))?;
            rag.client
                .remember(rag::MemoryEntry {
                    id: Uuid::new_v4(),
                    vector,
                    workspace_id: rag.workspace_id,
                    agent_key: agent_key.to_string(),
                    text: content.clone(),
                    created_at: chrono::Utc::now(),
                })
                .await
                .map_err(|e| format!("memory_write node failed: {e}"))?;

            Ok(serde_json::json!({ "remembered": content }))
        }

        "memory_read" => {
            let rag = rag.ok_or_else(|| {
                "no RAG context configured (Qdrant/embedding provider unavailable)".to_string()
            })?;

            let agent_key = node
                .config
                .get("agent_key")
                .and_then(Value::as_str)
                .ok_or_else(|| "memory_read node missing \"agent_key\" in config".to_string())?;
            let query_template = node
                .config
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "memory_read node missing \"query\" in config".to_string())?;
            let limit = node
                .config
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(5);

            let context = merge_inputs(inputs);
            let query = render_template(query_template, &context);

            let vector = rag
                .embedding_provider
                .embed(&query)
                .await
                .map_err(|e| format!("memory_read node embedding failed: {e}"))?;
            let hits = rag
                .client
                .recall(rag.workspace_id, agent_key, vector, limit)
                .await
                .map_err(|e| format!("memory_read node recall failed: {e}"))?;

            let memories: Vec<Value> = hits
                .into_iter()
                .map(|hit| {
                    serde_json::json!({
                        "text": hit.text,
                        "created_at": hit.created_at.to_rfc3339(),
                        "score": hit.score,
                    })
                })
                .collect();

            Ok(serde_json::json!({ "memories": memories }))
        }

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

            let mut context = merge_inputs(inputs);
            if let Some(tool_specs) = node.config.get("tools").and_then(Value::as_array) {
                run_tool_calls(tool_specs, &mut context).await?;
            }
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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        let output = output_of(&result, join);
        let inputs = output.get("inputs").and_then(Value::as_array).unwrap();
        assert_eq!(inputs.len(), 2);
    }

    #[tokio::test]
    async fn unknown_node_type_fails() {
        let a = Uuid::new_v4();
        let nodes = vec![node(a, "bogus", Value::Null)];
        let result = execute(&nodes, &[], None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, None, None, None, None).await;

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

        let result = execute(&nodes, &edges, Some(&EchoProvider), None, None, None).await;

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

        let result = execute(&nodes, &[], None, None, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
    }

    #[tokio::test]
    async fn approval_node_pauses_execution_and_blocks_downstream() {
        let input = Uuid::new_v4();
        let gate = Uuid::new_v4();
        let downstream = Uuid::new_v4();

        let nodes = vec![
            node(input, "input", serde_json::json!({ "value": { "x": 1 } })),
            node(gate, "approval", Value::Null),
            node(
                downstream,
                "transform",
                serde_json::json!({ "merge": { "y": 2 } }),
            ),
        ];
        let edges = vec![edge(input, gate, None), edge(gate, downstream, None)];

        let result = execute(&nodes, &edges, None, None, None, None).await;

        assert_eq!(result.status, ExecutionStatus::Waiting);
        assert_eq!(status_of(&result, gate), NodeStatus::Waiting);
        // Blocked behind the gate: not run, not reported as a bogus "cycle" failure either.
        assert!(result.nodes.iter().all(|n| n.node_id != downstream));
    }

    #[tokio::test]
    async fn approved_resume_replays_upstream_and_runs_downstream() {
        let input = Uuid::new_v4();
        let gate = Uuid::new_v4();
        let downstream = Uuid::new_v4();

        let nodes = vec![
            node(input, "input", serde_json::json!({ "value": { "x": 1 } })),
            node(gate, "approval", Value::Null),
            node(
                downstream,
                "transform",
                serde_json::json!({ "merge": { "y": 2 } }),
            ),
        ];
        let edges = vec![edge(input, gate, None), edge(gate, downstream, None)];

        let first = execute(&nodes, &edges, None, None, None, None).await;
        let seed_results = first.nodes.clone();

        let mut approval_decisions = HashMap::new();
        approval_decisions.insert(gate, true);
        let resume = ResumeState {
            seed_results,
            approval_decisions,
        };

        let second = execute(&nodes, &edges, None, None, Some(&resume), None).await;

        assert_eq!(second.status, ExecutionStatus::Succeeded);
        assert_eq!(status_of(&second, gate), NodeStatus::Succeeded);
        assert_eq!(status_of(&second, downstream), NodeStatus::Succeeded);
        assert_eq!(
            *output_of(&second, downstream),
            serde_json::json!({ "x": 1, "y": 2 })
        );
    }

    #[tokio::test]
    async fn rejected_resume_fails_the_gate_and_skips_downstream() {
        let input = Uuid::new_v4();
        let gate = Uuid::new_v4();
        let downstream = Uuid::new_v4();

        let nodes = vec![
            node(input, "input", serde_json::json!({ "value": { "x": 1 } })),
            node(gate, "approval", Value::Null),
            node(downstream, "transform", Value::Null),
        ];
        let edges = vec![edge(input, gate, None), edge(gate, downstream, None)];

        let first = execute(&nodes, &edges, None, None, None, None).await;

        let mut approval_decisions = HashMap::new();
        approval_decisions.insert(gate, false);
        let resume = ResumeState {
            seed_results: first.nodes.clone(),
            approval_decisions,
        };

        let second = execute(&nodes, &edges, None, None, Some(&resume), None).await;

        assert_eq!(second.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&second, gate), NodeStatus::Failed);
        assert_eq!(status_of(&second, downstream), NodeStatus::Skipped);
    }

    #[tokio::test]
    async fn seeded_nodes_are_not_recomputed_on_resume() {
        // A provider that errors on every call: if the already-succeeded agent node were
        // recomputed during resume instead of replayed, this would flip its status.
        struct FailingProvider;
        #[async_trait::async_trait]
        impl LlmProvider for FailingProvider {
            async fn complete(&self, _: &CompletionRequest) -> Result<String, llm::LlmError> {
                Err(llm::LlmError::UnknownProvider(
                    "should not be called".into(),
                ))
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }

        let agent = Uuid::new_v4();
        let gate = Uuid::new_v4();
        let nodes = vec![
            node(
                agent,
                "agent",
                serde_json::json!({ "prompt": "hi", "model": "test-model" }),
            ),
            node(gate, "approval", Value::Null),
        ];
        let edges = vec![edge(agent, gate, None)];

        let first = execute(&nodes, &edges, Some(&EchoProvider), None, None, None).await;
        assert_eq!(status_of(&first, agent), NodeStatus::Succeeded);

        let mut approval_decisions = HashMap::new();
        approval_decisions.insert(gate, true);
        let resume = ResumeState {
            seed_results: first.nodes.clone(),
            approval_decisions,
        };

        let second = execute(
            &nodes,
            &edges,
            Some(&FailingProvider),
            None,
            Some(&resume),
            None,
        )
        .await;

        assert_eq!(second.status, ExecutionStatus::Succeeded);
        assert_eq!(status_of(&second, agent), NodeStatus::Succeeded);
    }
}

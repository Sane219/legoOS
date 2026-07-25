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

/// Runs a workflow graph to completion synchronously, in-process (no queue/workers yet).
///
/// Nodes are processed in topological order. A node whose inbound edges never fire (its
/// upstream failed, or a condition edge didn't match) is marked `Skipped` rather than run,
/// and that skip propagates downstream unless another path also feeds the same node.
pub fn execute(nodes: &[Node], edges: &[Edge]) -> ExecutionResult {
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
            results.push(NodeResult {
                node_id,
                status: NodeStatus::Skipped,
                output: None,
                error: None,
            });
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

        match run_node(node, &inputs) {
            Ok(output) => {
                propagate(
                    node_id,
                    Some(&output),
                    &outbound,
                    &mut unresolved,
                    &mut fired_inbound,
                    &mut queue,
                );
                results.push(NodeResult {
                    node_id,
                    status: NodeStatus::Succeeded,
                    output: Some(output),
                    error: None,
                });
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
                results.push(NodeResult {
                    node_id,
                    status: NodeStatus::Failed,
                    output: None,
                    error: Some(err),
                });
            }
        }
    }

    for n in nodes {
        if !visited.contains(&n.id) {
            results.push(NodeResult {
                node_id: n.id,
                status: NodeStatus::Failed,
                output: None,
                error: Some("node was never resolved (cycle in workflow graph?)".into()),
            });
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

fn run_node(node: &Node, inputs: &[Value]) -> Result<Value, String> {
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

    #[test]
    fn linear_chain_propagates_and_merges() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        let nodes = vec![
            node(a, "input", serde_json::json!({ "value": { "x": 1 } })),
            node(b, "transform", serde_json::json!({ "merge": { "y": 2 } })),
            node(c, "transform", serde_json::json!({ "merge": { "z": 3 } })),
        ];
        let edges = vec![edge(a, b, None), edge(b, c, None)];

        let result = execute(&nodes, &edges);

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(
            *output_of(&result, c),
            serde_json::json!({ "x": 1, "y": 2, "z": 3 })
        );
    }

    #[test]
    fn condition_true_branch_runs_and_false_branch_is_skipped() {
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

        let result = execute(&nodes, &edges);

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(status_of(&result, on_true), NodeStatus::Succeeded);
        assert_eq!(status_of(&result, on_false), NodeStatus::Skipped);
    }

    #[test]
    fn condition_false_branch_runs_and_true_branch_is_skipped() {
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

        let result = execute(&nodes, &edges);

        assert_eq!(status_of(&result, on_true), NodeStatus::Skipped);
        assert_eq!(status_of(&result, on_false), NodeStatus::Succeeded);
    }

    #[test]
    fn condition_missing_field_defaults_to_false() {
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

        let result = execute(&nodes, &edges);

        assert_eq!(
            *output_of(&result, cond),
            serde_json::json!({ "result": false })
        );
    }

    #[test]
    fn fan_in_collects_all_inputs() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let join = Uuid::new_v4();

        let nodes = vec![
            node(a, "input", serde_json::json!({ "value": "from-a" })),
            node(b, "input", serde_json::json!({ "value": "from-b" })),
            node(join, "transform", Value::Null),
        ];
        let edges = vec![edge(a, join, None), edge(b, join, None)];

        let result = execute(&nodes, &edges);

        assert_eq!(result.status, ExecutionStatus::Succeeded);
        let output = output_of(&result, join);
        let inputs = output.get("inputs").and_then(Value::as_array).unwrap();
        assert_eq!(inputs.len(), 2);
    }

    #[test]
    fn unknown_node_type_fails() {
        let a = Uuid::new_v4();
        let nodes = vec![node(a, "bogus", Value::Null)];
        let result = execute(&nodes, &[]);

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
    }

    #[test]
    fn upstream_failure_skips_downstream_but_marks_execution_failed() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let nodes = vec![
            node(a, "bogus", Value::Null),
            node(b, "transform", Value::Null),
        ];
        let edges = vec![edge(a, b, None)];

        let result = execute(&nodes, &edges);

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
        assert_eq!(status_of(&result, b), NodeStatus::Skipped);
    }

    #[test]
    fn cycle_leaves_nodes_unresolved_and_fails() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let nodes = vec![
            node(a, "transform", Value::Null),
            node(b, "transform", Value::Null),
        ];
        let edges = vec![edge(a, b, None), edge(b, a, None)];

        let result = execute(&nodes, &edges);

        assert_eq!(result.status, ExecutionStatus::Failed);
        assert_eq!(status_of(&result, a), NodeStatus::Failed);
        assert_eq!(status_of(&result, b), NodeStatus::Failed);
    }
}

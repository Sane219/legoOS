use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Redis stream that workflow-run jobs are published to.
pub const WORKFLOW_RUNS_STREAM: &str = "workflow_runs";

/// Consumer group all worker processes share so a stream entry is claimed by exactly one worker.
pub const WORKFLOW_RUNS_GROUP: &str = "workers";

/// Where jobs land after failing `MAX_DELIVERIES` times, instead of retrying forever.
pub const WORKFLOW_RUNS_DEAD_LETTER_STREAM: &str = "workflow_runs:dead-letter";

/// A pending entry idle longer than this (no ack, no progress) is assumed to belong to a
/// crashed worker and becomes eligible for another worker to reclaim.
pub const VISIBILITY_TIMEOUT_MS: i64 = 60_000;

/// Deliveries (original + reclaims) allowed before a job is routed to the dead-letter stream.
pub const MAX_DELIVERIES: i64 = 5;

/// Redis pub/sub channel prefix for per-node execution trace events.
/// The full channel name is `{TRACE_CHANNEL_PREFIX}{execution_id}`.
pub const TRACE_CHANNEL_PREFIX: &str = "execution-trace:";

pub fn trace_channel(execution_id: Uuid) -> String {
    format!("{TRACE_CHANNEL_PREFIX}{execution_id}")
}

/// Field name used to carry the JSON-encoded `RunJob` in each stream entry.
pub const JOB_FIELD: &str = "job";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunJob {
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
}

/// Published by a worker to `trace_channel(execution_id)` as each node finishes, and once
/// more (`Final`) when the whole execution completes, so a WebSocket subscriber knows to close.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEvent {
    NodeResult {
        node_id: Uuid,
        status: String,
        output: Option<Value>,
        error: Option<String>,
    },
    Final {
        status: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_channel_includes_execution_id() {
        let id = Uuid::nil();
        assert_eq!(
            trace_channel(id),
            "execution-trace:00000000-0000-0000-0000-000000000000"
        );
    }
}

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

/// Accepts a standard 5-field cron expression (`minute hour day-of-month month
/// day-of-week`, e.g. `"0 9 * * mon-fri"`) and returns the next fire time strictly after
/// `after`. Returns `Err` if `expr` doesn't parse.
///
/// The `cron` crate speaks 6/7-field expressions (seconds first, optional year last); we
/// convert rather than expose that dialect, since every cron expression a user has ever
/// seen (crontab, GitHub Actions, etc.) is 5-field.
pub fn next_run_after(
    expr: &str,
    after: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "expected a 5-field cron expression (minute hour day month weekday), got {} field(s)",
            fields.len()
        ));
    }
    let seven_field = format!(
        "0 {} {} {} {} {} *",
        fields[0], fields[1], fields[2], fields[3], fields[4]
    );

    let schedule: cron::Schedule = seven_field
        .parse()
        .map_err(|e| format!("invalid cron expression: {e}"))?;
    schedule
        .after(&after)
        .next()
        .ok_or_else(|| "cron expression has no future occurrences".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    #[test]
    fn trace_channel_includes_execution_id() {
        let id = Uuid::nil();
        assert_eq!(
            trace_channel(id),
            "execution-trace:00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn next_run_after_computes_the_next_daily_occurrence() {
        let after = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
        let next = next_run_after("0 9 * * *", after).unwrap();
        assert_eq!(
            next,
            chrono::Utc.with_ymd_and_hms(2026, 1, 2, 9, 0, 0).unwrap()
        );
    }

    #[test]
    fn next_run_after_rejects_wrong_field_count() {
        assert!(next_run_after("0 9 * *", chrono::Utc::now()).is_err());
        assert!(next_run_after("0 0 9 * * *", chrono::Utc::now()).is_err());
    }

    #[test]
    fn next_run_after_rejects_garbage() {
        assert!(next_run_after("not a cron", chrono::Utc::now()).is_err());
    }

    #[test]
    fn next_run_after_supports_weekday_ranges() {
        // Monday 2026-01-05 09:00 UTC, weekdays only.
        let after = chrono::Utc.with_ymd_and_hms(2026, 1, 3, 9, 0, 0).unwrap(); // a Saturday
        let next = next_run_after("0 9 * * mon-fri", after).unwrap();
        assert_eq!(next.weekday(), chrono::Weekday::Mon);
    }
}

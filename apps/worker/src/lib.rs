use queue::{
    JOB_FIELD, MAX_DELIVERIES, RunJob, TraceEvent, VISIBILITY_TIMEOUT_MS,
    WORKFLOW_RUNS_DEAD_LETTER_STREAM, WORKFLOW_RUNS_GROUP, WORKFLOW_RUNS_STREAM,
};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use redis::streams::{
    StreamClaimReply, StreamId, StreamPendingCountReply, StreamReadOptions, StreamReadReply,
};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub async fn ensure_group(conn: &mut ConnectionManager) -> anyhow::Result<()> {
    let result: redis::RedisResult<()> = conn
        .xgroup_create_mkstream(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, "0")
        .await;
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.to_string().contains("BUSYGROUP") => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn job_from_entry(entry: &StreamId) -> Option<RunJob> {
    entry
        .get::<String>(JOB_FIELD)
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Blocks up to `block_ms` for new stream entries assigned to `consumer`, returning
/// `(entry_id, job)` pairs. A malformed entry is acked (dropped) immediately rather than
/// returned, since retrying it can never succeed.
pub async fn read_new(
    conn: &mut ConnectionManager,
    consumer: &str,
    block_ms: usize,
) -> redis::RedisResult<Vec<(String, RunJob)>> {
    let opts = StreamReadOptions::default()
        .group(WORKFLOW_RUNS_GROUP, consumer)
        .block(block_ms)
        .count(10);
    let reply: StreamReadReply = conn
        .xread_options(&[WORKFLOW_RUNS_STREAM], &[">"], &opts)
        .await?;

    let mut jobs = Vec::new();
    for key in reply.keys {
        for entry in key.ids {
            match job_from_entry(&entry) {
                Some(job) => jobs.push((entry.id, job)),
                None => {
                    tracing::error!(entry_id = %entry.id, "malformed job entry, acking to drop it");
                    let _: redis::RedisResult<()> = conn
                        .xack(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, &[entry.id])
                        .await;
                }
            }
        }
    }
    Ok(jobs)
}

/// Reclaims entries some worker read but never acked, long enough ago that it's assumed
/// dead. Entries reclaimed past `MAX_DELIVERIES` are routed to the dead-letter stream
/// instead of being retried forever.
pub async fn reclaim_stuck(
    pool: &PgPool,
    conn: &mut ConnectionManager,
    consumer: &str,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
) -> anyhow::Result<()> {
    let pending: StreamPendingCountReply = conn
        .xpending_count(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, "-", "+", 50)
        .await?;

    for entry in pending.ids {
        if (entry.last_delivered_ms as i64) < VISIBILITY_TIMEOUT_MS {
            continue;
        }

        if entry.times_delivered as i64 > MAX_DELIVERIES {
            dead_letter(conn, &entry.id).await?;
            continue;
        }

        tracing::warn!(entry_id = %entry.id, deliveries = entry.times_delivered, "reclaiming stuck job");
        let claimed: StreamClaimReply = conn
            .xclaim(
                WORKFLOW_RUNS_STREAM,
                WORKFLOW_RUNS_GROUP,
                consumer,
                VISIBILITY_TIMEOUT_MS,
                std::slice::from_ref(&entry.id),
            )
            .await?;

        for claimed_entry in claimed.ids {
            match job_from_entry(&claimed_entry) {
                Some(job) => process_entry(pool, conn, &claimed_entry.id, job, provider).await,
                None => {
                    let _: redis::RedisResult<()> = conn
                        .xack(
                            WORKFLOW_RUNS_STREAM,
                            WORKFLOW_RUNS_GROUP,
                            &[claimed_entry.id],
                        )
                        .await;
                }
            }
        }
    }

    Ok(())
}

async fn dead_letter(conn: &mut ConnectionManager, entry_id: &str) -> anyhow::Result<()> {
    tracing::error!(
        entry_id,
        "job exceeded max deliveries, moving to dead-letter stream"
    );
    let _: String = conn
        .xadd(
            WORKFLOW_RUNS_DEAD_LETTER_STREAM,
            "*",
            &[("original_id", entry_id)],
        )
        .await?;
    let _: () = conn
        .xack(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, &[entry_id])
        .await?;
    Ok(())
}

/// Runs `job` to completion and acks its stream entry regardless of outcome: a workflow
/// that fails at the node level is a normal, fully-recorded execution, not a reason to
/// retry. Only an infrastructure error (DB/redis) surfaces here for the caller to log.
pub async fn process_entry(
    pool: &PgPool,
    redis: &mut ConnectionManager,
    entry_id: &str,
    job: RunJob,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
) {
    tracing::info!(execution_id = %job.execution_id, "processing workflow run");
    if let Err(e) = run_job(pool, redis, &job, provider).await {
        tracing::error!(execution_id = %job.execution_id, error = %e, "workflow run failed");
    }
    let _: redis::RedisResult<()> = redis
        .xack(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, &[entry_id])
        .await;
}

fn to_trace_event(result: &executor::NodeResult) -> TraceEvent {
    let status = match result.status {
        executor::NodeStatus::Succeeded => "succeeded",
        executor::NodeStatus::Failed => "failed",
        executor::NodeStatus::Skipped => "skipped",
    };
    TraceEvent::NodeResult {
        node_id: result.node_id,
        status: status.to_string(),
        output: result.output.clone(),
        error: result.error.clone(),
    }
}

pub async fn run_job(
    pool: &PgPool,
    redis: &mut ConnectionManager,
    job: &RunJob,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE workflow_executions SET status = 'running' WHERE id = $1")
        .bind(job.execution_id)
        .execute(pool)
        .await?;

    let node_rows = sqlx::query_as::<_, (Uuid, String, Value)>(
        "SELECT id, node_type, config FROM workflow_nodes WHERE workflow_id = $1",
    )
    .bind(job.workflow_id)
    .fetch_all(pool)
    .await?;

    let edge_rows = sqlx::query_as::<_, (Uuid, Uuid, Option<String>)>(
        "SELECT source_node_id, target_node_id, condition FROM workflow_edges WHERE workflow_id = $1",
    )
    .bind(job.workflow_id)
    .fetch_all(pool)
    .await?;

    let nodes: Vec<executor::Node> = node_rows
        .into_iter()
        .map(|(id, node_type, config)| executor::Node {
            id,
            node_type,
            config,
        })
        .collect();
    let edges: Vec<executor::Edge> = edge_rows
        .into_iter()
        .map(|(source, target, condition)| executor::Edge {
            source,
            target,
            condition,
        })
        .collect();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<executor::NodeResult>();
    let channel = queue::trace_channel(job.execution_id);
    let mut publisher_redis = redis.clone();
    let publisher_channel = channel.clone();
    let publisher = tokio::spawn(async move {
        while let Some(result) = rx.recv().await {
            let event = serde_json::to_string(&to_trace_event(&result)).expect("serializes");
            let _: redis::RedisResult<()> =
                publisher_redis.publish(&publisher_channel, event).await;
        }
    });

    let provider_ref = provider.map(Arc::as_ref);
    let result = executor::execute(&nodes, &edges, provider_ref, Some(&tx)).await;
    drop(tx);
    let _ = publisher.await;

    let status_str = match result.status {
        executor::ExecutionStatus::Succeeded => "succeeded",
        executor::ExecutionStatus::Failed => "failed",
    };

    let mut db_tx = pool.begin().await?;
    sqlx::query("UPDATE workflow_executions SET status = $1, finished_at = now() WHERE id = $2")
        .bind(status_str)
        .bind(job.execution_id)
        .execute(&mut *db_tx)
        .await?;

    for node_result in &result.nodes {
        let node_status_str = match node_result.status {
            executor::NodeStatus::Succeeded => "succeeded",
            executor::NodeStatus::Failed => "failed",
            executor::NodeStatus::Skipped => "skipped",
        };

        sqlx::query(
            "INSERT INTO workflow_execution_nodes (execution_id, node_id, status, output, error)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(job.execution_id)
        .bind(node_result.node_id)
        .bind(node_status_str)
        .bind(&node_result.output)
        .bind(&node_result.error)
        .execute(&mut *db_tx)
        .await?;
    }

    db_tx.commit().await?;

    let final_event = serde_json::to_string(&TraceEvent::Final {
        status: status_str.to_string(),
    })
    .expect("serializes");
    let _: redis::RedisResult<()> = redis.publish(&channel, final_event).await;

    Ok(())
}

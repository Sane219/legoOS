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
#[allow(clippy::too_many_arguments)]
pub async fn reclaim_stuck(
    pool: &PgPool,
    conn: &mut ConnectionManager,
    consumer: &str,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
    mcp_credential_key: &str,
    rag_client: Option<&rag::RagClient>,
    embedding_provider: Option<&Arc<dyn llm::EmbeddingProvider>>,
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
                Some(job) => {
                    process_entry(
                        pool,
                        conn,
                        &claimed_entry.id,
                        job,
                        provider,
                        mcp_credential_key,
                        rag_client,
                        embedding_provider,
                    )
                    .await
                }
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
#[allow(clippy::too_many_arguments)]
pub async fn process_entry(
    pool: &PgPool,
    redis: &mut ConnectionManager,
    entry_id: &str,
    job: RunJob,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
    mcp_credential_key: &str,
    rag_client: Option<&rag::RagClient>,
    embedding_provider: Option<&Arc<dyn llm::EmbeddingProvider>>,
) {
    tracing::info!(execution_id = %job.execution_id, "processing workflow run");
    if let Err(e) = run_job(
        pool,
        redis,
        &job,
        provider,
        mcp_credential_key,
        rag_client,
        embedding_provider,
    )
    .await
    {
        tracing::error!(execution_id = %job.execution_id, error = %e, "workflow run failed");
    }
    let _: redis::RedisResult<()> = redis
        .xack(WORKFLOW_RUNS_STREAM, WORKFLOW_RUNS_GROUP, &[entry_id])
        .await;
}

/// Agent node `tools` entries can reference a saved workspace MCP connection by id
/// (`"mcp_connection_id"`) instead of embedding a raw URL/token in the workflow config.
/// This resolves each one to the `mcp_url`/`mcp_token` fields `executor` actually reads,
/// decrypting the stored token — scoped to `workspace_id` so a workflow can't reach
/// another workspace's connection by guessing its id.
async fn resolve_mcp_connections(
    pool: &PgPool,
    workspace_id: Uuid,
    mcp_credential_key: &str,
    nodes: &mut [executor::Node],
) -> anyhow::Result<()> {
    for node in nodes.iter_mut() {
        if node.node_type != "agent" {
            continue;
        }
        let Some(tools) = node.config.get_mut("tools").and_then(Value::as_array_mut) else {
            continue;
        };

        for spec in tools.iter_mut() {
            let Some(connection_id) = spec
                .get("mcp_connection_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };

            let row = sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT url, encrypted_bearer_token FROM mcp_connections
                 WHERE id = $1 AND workspace_id = $2",
            )
            .bind(connection_id)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "mcp connection {connection_id} not found in workspace {workspace_id}"
                )
            })?;

            let (url, encrypted_token) = row;
            let token = encrypted_token
                .map(|t| mcp::decrypt_token(mcp_credential_key, &t))
                .transpose()
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            if let Some(map) = spec.as_object_mut() {
                map.insert("mcp_url".to_string(), Value::String(url));
                if let Some(token) = token {
                    map.insert("mcp_token".to_string(), Value::String(token));
                }
            }
        }
    }
    Ok(())
}

fn node_status_str(status: executor::NodeStatus) -> &'static str {
    match status {
        executor::NodeStatus::Succeeded => "succeeded",
        executor::NodeStatus::Failed => "failed",
        executor::NodeStatus::Skipped => "skipped",
        executor::NodeStatus::Waiting => "waiting",
    }
}

fn to_trace_event(result: &executor::NodeResult) -> TraceEvent {
    TraceEvent::NodeResult {
        node_id: result.node_id,
        status: node_status_str(result.status).to_string(),
        output: result.output.clone(),
        error: result.error.clone(),
    }
}

/// Loads everything needed to resume a possibly-paused execution: prior node results
/// (excluding any still `waiting` — the gate they belong to is re-evaluated fresh below,
/// not replayed) and decisions for any approval gates a human has already acted on.
async fn load_resume_state(
    pool: &PgPool,
    execution_id: Uuid,
) -> anyhow::Result<executor::ResumeState> {
    let seed_rows = sqlx::query_as::<_, (Uuid, String, Option<Value>, Option<String>)>(
        "SELECT node_id, status, output, error FROM workflow_execution_nodes
         WHERE execution_id = $1 AND status != 'waiting'",
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await?;

    let seed_results = seed_rows
        .into_iter()
        .map(|(node_id, status, output, error)| executor::NodeResult {
            node_id,
            status: match status.as_str() {
                "succeeded" => executor::NodeStatus::Succeeded,
                "skipped" => executor::NodeStatus::Skipped,
                _ => executor::NodeStatus::Failed,
            },
            output,
            error,
        })
        .collect();

    let decision_rows = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT node_id, status FROM approval_gates
         WHERE execution_id = $1 AND status != 'pending'",
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await?;

    let approval_decisions = decision_rows
        .into_iter()
        .map(|(node_id, status)| (node_id, status == "approved"))
        .collect();

    Ok(executor::ResumeState {
        seed_results,
        approval_decisions,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn run_job(
    pool: &PgPool,
    redis: &mut ConnectionManager,
    job: &RunJob,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
    mcp_credential_key: &str,
    rag_client: Option<&rag::RagClient>,
    embedding_provider: Option<&Arc<dyn llm::EmbeddingProvider>>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE workflow_executions SET status = 'running' WHERE id = $1")
        .bind(job.execution_id)
        .execute(pool)
        .await?;

    let workspace_id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM workflows WHERE id = $1")
        .bind(job.workflow_id)
        .fetch_one(pool)
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

    let mut nodes: Vec<executor::Node> = node_rows
        .into_iter()
        .map(|(id, node_type, config)| executor::Node {
            id,
            node_type,
            config,
        })
        .collect();
    resolve_mcp_connections(pool, workspace_id, mcp_credential_key, &mut nodes).await?;

    let edges: Vec<executor::Edge> = edge_rows
        .into_iter()
        .map(|(source, target, condition)| executor::Edge {
            source,
            target,
            condition,
        })
        .collect();

    let resume_state = load_resume_state(pool, job.execution_id).await?;

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
    let rag_context = match (rag_client, embedding_provider) {
        (Some(client), Some(embedding_provider)) => Some(executor::RagContext {
            client,
            embedding_provider: embedding_provider.as_ref(),
            workspace_id,
        }),
        _ => None,
    };
    let result = executor::execute(
        &nodes,
        &edges,
        provider_ref,
        Some(&tx),
        Some(&resume_state),
        rag_context.as_ref(),
    )
    .await;
    drop(tx);
    let _ = publisher.await;

    let status_str = match result.status {
        executor::ExecutionStatus::Succeeded => "succeeded",
        executor::ExecutionStatus::Failed => "failed",
        executor::ExecutionStatus::Waiting => "waiting",
    };

    let mut db_tx = pool.begin().await?;
    if result.status == executor::ExecutionStatus::Waiting {
        sqlx::query("UPDATE workflow_executions SET status = $1 WHERE id = $2")
            .bind(status_str)
            .bind(job.execution_id)
            .execute(&mut *db_tx)
            .await?;
    } else {
        sqlx::query(
            "UPDATE workflow_executions SET status = $1, finished_at = now() WHERE id = $2",
        )
        .bind(status_str)
        .bind(job.execution_id)
        .execute(&mut *db_tx)
        .await?;
    }

    for node_result in &result.nodes {
        sqlx::query(
            "INSERT INTO workflow_execution_nodes (execution_id, node_id, status, output, error)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (execution_id, node_id)
             DO UPDATE SET status = excluded.status, output = excluded.output, error = excluded.error",
        )
        .bind(job.execution_id)
        .bind(node_result.node_id)
        .bind(node_status_str(node_result.status))
        .bind(&node_result.output)
        .bind(&node_result.error)
        .execute(&mut *db_tx)
        .await?;

        if node_result.status == executor::NodeStatus::Waiting {
            sqlx::query(
                "INSERT INTO approval_gates (execution_id, node_id)
                 VALUES ($1, $2)
                 ON CONFLICT (execution_id, node_id) DO NOTHING",
            )
            .bind(job.execution_id)
            .bind(node_result.node_id)
            .execute(&mut *db_tx)
            .await?;
        }
    }

    db_tx.commit().await?;

    let final_event = serde_json::to_string(&TraceEvent::Final {
        status: status_str.to_string(),
    })
    .expect("serializes");
    let _: redis::RedisResult<()> = redis.publish(&channel, final_event).await;

    Ok(())
}

use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser, error::AppError, state::AppState, workflows::ensure_workflow_exists,
    workspaces::member_role,
};

/// Streams `queue::TraceEvent`s for one execution as JSON text frames: first a replay of
/// any node results already persisted (covers events published before the client
/// connected), then live events off the worker's pub/sub channel until a `Final` event,
/// or immediately a `Final` if the execution had already finished.
pub async fn execution_trace(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id, execution_id)): Path<(Uuid, Uuid, Uuid)>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM workflow_executions WHERE id = $1 AND workflow_id = $2)",
    )
    .bind(execution_id)
    .bind(workflow_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;
    if !exists {
        return Err(AppError::NotFound("execution not found".into()));
    }

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = run_trace_socket(socket, state, execution_id).await {
            tracing::warn!(execution_id = %execution_id, error = %e, "trace socket ended with an error");
        }
    }))
}

async fn run_trace_socket(
    mut socket: WebSocket,
    state: AppState,
    execution_id: Uuid,
) -> anyhow::Result<()> {
    // Subscribe before reading any state, so a Final published between our subscribe and
    // our snapshot query still lands in the stream instead of being missed entirely.
    let mut pubsub = state.redis_client.get_async_pubsub().await?;
    pubsub.subscribe(queue::trace_channel(execution_id)).await?;
    let mut stream = pubsub.into_on_message();

    let existing_nodes = sqlx::query_as::<_, (Uuid, String, Option<Value>, Option<String>)>(
        "SELECT node_id, status, output, error FROM workflow_execution_nodes WHERE execution_id = $1",
    )
    .bind(execution_id)
    .fetch_all(&state.pool)
    .await?;

    for (node_id, status, output, error) in existing_nodes {
        let event = queue::TraceEvent::NodeResult {
            node_id,
            status,
            output,
            error,
        };
        socket
            .send(Message::Text(serde_json::to_string(&event)?.into()))
            .await?;
    }

    let status: String = sqlx::query_scalar("SELECT status FROM workflow_executions WHERE id = $1")
        .bind(execution_id)
        .fetch_one(&state.pool)
        .await?;
    if status != "pending" && status != "running" {
        let event = queue::TraceEvent::Final { status };
        socket
            .send(Message::Text(serde_json::to_string(&event)?.into()))
            .await?;
        return Ok(());
    }

    loop {
        tokio::select! {
            msg = stream.next() => {
                let Some(msg) = msg else { break };
                let payload: String = msg.get_payload()?;
                let is_final = serde_json::from_str::<queue::TraceEvent>(&payload)
                    .is_ok_and(|event| matches!(event, queue::TraceEvent::Final { .. }));
                socket.send(Message::Text(payload.into())).await?;
                if is_final {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => continue,
                    _ => break,
                }
            }
        }
    }

    Ok(())
}

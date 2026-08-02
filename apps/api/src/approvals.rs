use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser, error::AppError, models::ApprovalGateResponse, state::AppState,
    workspaces::member_role,
};

/// Every pending approval gate across the workspace's workflows — the "inbox" a member
/// works through, each with the context (merged upstream input) the gate paused on.
pub async fn list_approvals(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<ApprovalGateResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let rows = sqlx::query_as::<_, crate::models::ApprovalGateRow>(
        "SELECT ag.id, ag.execution_id, e.workflow_id, w.name AS workflow_name,
                ag.node_id, n.output AS context, ag.status, ag.created_at
         FROM approval_gates ag
         JOIN workflow_executions e ON e.id = ag.execution_id
         JOIN workflows w ON w.id = e.workflow_id
         LEFT JOIN workflow_execution_nodes n
             ON n.execution_id = ag.execution_id AND n.node_id = ag.node_id
         WHERE w.workspace_id = $1 AND ag.status = 'pending'
         ORDER BY ag.created_at",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

async fn decide(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Uuid,
    gate_id: Uuid,
    decision: &str,
) -> Result<(), AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let resolved = sqlx::query_as::<_, (Uuid, Uuid)>(
        "UPDATE approval_gates ag
         SET status = $1, decided_by = $2, decided_at = now()
         FROM workflow_executions e
         JOIN workflows w ON w.id = e.workflow_id
         WHERE ag.id = $3
           AND ag.execution_id = e.id
           AND w.workspace_id = $4
           AND ag.status = 'pending'
         RETURNING ag.execution_id, e.workflow_id",
    )
    .bind(decision)
    .bind(user_id)
    .bind(gate_id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("pending approval gate not found".into()))?;

    let (execution_id, workflow_id) = resolved;
    let job = queue::RunJob {
        execution_id,
        workflow_id,
    };
    let job_json = serde_json::to_string(&job).expect("RunJob always serializes");

    let mut redis = state.redis.clone();
    redis::cmd("XADD")
        .arg(queue::WORKFLOW_RUNS_STREAM)
        .arg("MAXLEN")
        .arg("~")
        .arg(queue::STREAM_MAXLEN)
        .arg("*")
        .arg(queue::JOB_FIELD)
        .arg(job_json)
        .query_async::<()>(&mut redis)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(())
}

pub async fn approve(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, gate_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    decide(&state, user_id, workspace_id, gate_id, "approved").await?;
    Ok(Json(serde_json::json!({ "status": "approved" })))
}

pub async fn reject(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, gate_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    decide(&state, user_id, workspace_id, gate_id, "rejected").await?;
    Ok(Json(serde_json::json!({ "status": "rejected" })))
}

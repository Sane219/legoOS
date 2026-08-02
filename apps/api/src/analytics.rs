use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser, error::AppError, models::ExecutionAnalyticsResponse, state::AppState,
    workflows::ensure_workflow_exists, workspaces::member_role,
};

/// Cost and eval-score trend for a workflow's most recent executions, newest first.
/// Costs/tokens are summed across every node in an execution; eval score is averaged
/// across that execution's `evaluate` nodes (NULL when it has none).
pub async fn workflow_analytics(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ExecutionAnalyticsResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let rows = sqlx::query_as::<_, ExecutionAnalyticsResponse>(
        "SELECT
             e.id AS execution_id,
             e.status,
             e.started_at,
             COALESCE(SUM((n.output->>'cost_usd')::double precision), 0) AS total_cost_usd,
             COALESCE(SUM((n.output->>'input_tokens')::bigint), 0) AS total_input_tokens,
             COALESCE(SUM((n.output->>'output_tokens')::bigint), 0) AS total_output_tokens,
             AVG((n.output->>'score')::double precision) AS avg_eval_score
         FROM workflow_executions e
         LEFT JOIN workflow_execution_nodes n ON n.execution_id = e.id
         WHERE e.workflow_id = $1
         GROUP BY e.id, e.status, e.started_at
         ORDER BY e.started_at DESC
         LIMIT 50",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows))
}

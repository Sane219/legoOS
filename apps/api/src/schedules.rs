use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    models::{ScheduleResponse, ScheduleRow},
    state::AppState,
    workflows::ensure_workflow_exists,
    workspaces::{member_role, require_role},
};

#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub cron_expression: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

pub async fn create_schedule(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateScheduleRequest>,
) -> Result<Json<ScheduleResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let next_run_at =
        queue::next_run_after(&body.cron_expression, Utc::now()).map_err(AppError::Validation)?;

    let row = sqlx::query_as::<_, ScheduleRow>(
        "INSERT INTO workflow_schedules (workflow_id, cron_expression, enabled, next_run_at)
         VALUES ($1, $2, $3, $4)
         RETURNING id, workflow_id, cron_expression, enabled, next_run_at, last_run_at, created_at",
    )
    .bind(workflow_id)
    .bind(&body.cron_expression)
    .bind(body.enabled)
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(row.into()))
}

pub async fn list_schedules(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ScheduleResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let rows = sqlx::query_as::<_, ScheduleRow>(
        "SELECT id, workflow_id, cron_expression, enabled, next_run_at, last_run_at, created_at
         FROM workflow_schedules WHERE workflow_id = $1 ORDER BY created_at",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateScheduleRequest {
    pub enabled: bool,
}

pub async fn update_schedule(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id, schedule_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<UpdateScheduleRequest>,
) -> Result<Json<ScheduleResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    // Re-enabling schedules the next run relative to now, not whatever's stale in the row
    // (it may have sat disabled for a long time — that shouldn't cause an immediate
    // backlog of "overdue" firings).
    let row = if body.enabled {
        let cron_expression: String = sqlx::query_scalar(
            "SELECT cron_expression FROM workflow_schedules WHERE id = $1 AND workflow_id = $2",
        )
        .bind(schedule_id)
        .bind(workflow_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::NotFound("schedule not found".into()))?;
        let next_run_at =
            queue::next_run_after(&cron_expression, Utc::now()).map_err(AppError::Validation)?;

        sqlx::query_as::<_, ScheduleRow>(
            "UPDATE workflow_schedules SET enabled = true, next_run_at = $1
             WHERE id = $2 AND workflow_id = $3
             RETURNING id, workflow_id, cron_expression, enabled, next_run_at, last_run_at, created_at",
        )
        .bind(next_run_at)
        .bind(schedule_id)
        .bind(workflow_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        sqlx::query_as::<_, ScheduleRow>(
            "UPDATE workflow_schedules SET enabled = false
             WHERE id = $1 AND workflow_id = $2
             RETURNING id, workflow_id, cron_expression, enabled, next_run_at, last_run_at, created_at",
        )
        .bind(schedule_id)
        .bind(workflow_id)
        .fetch_optional(&state.pool)
        .await
    }
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("schedule not found".into()))?;

    Ok(Json(row.into()))
}

pub async fn delete_schedule(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id, schedule_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let result = sqlx::query("DELETE FROM workflow_schedules WHERE id = $1 AND workflow_id = $2")
        .bind(schedule_id)
        .bind(workflow_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("schedule not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

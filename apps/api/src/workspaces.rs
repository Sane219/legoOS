use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    models::{MemberResponse, MemberRow, WorkspaceResponse, WorkspaceWithRole},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub email: String,
}

async fn member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<String>, AppError> {
    let row = sqlx::query_as::<_, (String,)>(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(row.map(|(role,)| role))
}

pub async fn create_workspace(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, AppError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "workspace name must not be empty".into(),
        ));
    }

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let (id, created_at) = sqlx::query_as::<_, (Uuid, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO workspaces (name) VALUES ($1) RETURNING id, created_at",
    )
    .bind(name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(WorkspaceResponse {
        id,
        name: name.to_string(),
        created_at,
        role: "owner".to_string(),
    }))
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<Vec<WorkspaceResponse>>, AppError> {
    let rows = sqlx::query_as::<_, WorkspaceWithRole>(
        "SELECT w.id, w.name, w.created_at, wm.role
         FROM workspaces w
         JOIN workspace_members wm ON wm.workspace_id = w.id
         WHERE wm.user_id = $1
         ORDER BY w.created_at",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn get_workspace(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceResponse>, AppError> {
    let row = sqlx::query_as::<_, WorkspaceWithRole>(
        "SELECT w.id, w.name, w.created_at, wm.role
         FROM workspaces w
         JOIN workspace_members wm ON wm.workspace_id = w.id
         WHERE w.id = $1 AND wm.user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    Ok(Json(row.into()))
}

pub async fn list_members(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<MemberResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let members = sqlx::query_as::<_, MemberRow>(
        "SELECT u.id AS user_id, u.email, wm.role, wm.joined_at
         FROM workspace_members wm
         JOIN users u ON u.id = wm.user_id
         WHERE wm.workspace_id = $1
         ORDER BY wm.joined_at",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(members.into_iter().map(Into::into).collect()))
}

pub async fn add_member(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<Json<MemberResponse>, AppError> {
    let role = member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    if role != "owner" {
        return Err(AppError::Forbidden(
            "only the workspace owner can add members".into(),
        ));
    }

    let target_user_id = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or_else(|| AppError::Validation("no user exists with that email".into()))?
        .0;

    let (joined_at,) = sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>,)>(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')
         RETURNING joined_at",
    )
    .bind(workspace_id)
    .bind(target_user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            AppError::Conflict("user is already a member of this workspace".into())
        }
        _ => AppError::Internal(e.into()),
    })?;

    Ok(Json(MemberResponse {
        user_id: target_user_id,
        email: body.email,
        role: "member".to_string(),
        joined_at,
    }))
}

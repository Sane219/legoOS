use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    models::McpConnectionResponse,
    state::AppState,
    workspaces::{member_role, require_role},
};

#[derive(Debug, Deserialize)]
pub struct CreateMcpConnectionRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub bearer_token: Option<String>,
}

pub async fn create_connection(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateMcpConnectionRequest>,
) -> Result<Json<McpConnectionResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "connection name must not be empty".into(),
        ));
    }
    let url = body.url.trim();
    if url.is_empty() || (!url.starts_with("http://") && !url.starts_with("https://")) {
        return Err(AppError::Validation(
            "url must be an absolute http(s) URL".into(),
        ));
    }

    let encrypted_token = body
        .bearer_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|token| mcp::encrypt_token(&state.mcp_credential_key, token))
        .transpose()
        .map_err(|e| AppError::Internal(e.into()))?;

    let row = sqlx::query_as::<_, (Uuid, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO mcp_connections (workspace_id, name, url, encrypted_bearer_token)
         VALUES ($1, $2, $3, $4)
         RETURNING id, created_at",
    )
    .bind(workspace_id)
    .bind(name)
    .bind(url)
    .bind(&encrypted_token)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
            AppError::Validation("a connection with this name already exists".into())
        }
        e => AppError::Internal(e.into()),
    })?;

    Ok(Json(McpConnectionResponse {
        id: row.0,
        name: name.to_string(),
        url: url.to_string(),
        has_token: encrypted_token.is_some(),
        created_at: row.1,
    }))
}

pub async fn list_connections(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<McpConnectionResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let rows = sqlx::query_as::<_, crate::models::McpConnectionRow>(
        "SELECT id, name, url, encrypted_bearer_token IS NOT NULL AS has_token, created_at
         FROM mcp_connections WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn delete_connection(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;

    let result = sqlx::query("DELETE FROM mcp_connections WHERE id = $1 AND workspace_id = $2")
        .bind(connection_id)
        .bind(workspace_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("MCP connection not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Serialize)]
pub struct McpToolResponse {
    pub name: String,
    pub description: Option<String>,
}

/// Connects to the MCP server right now and lists its tools — both a "does this
/// connection actually work" check and how the workflow canvas discovers tool names to
/// wire into an agent node's `tools` config.
pub async fn test_connection(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, connection_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<McpToolResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let row = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT url, encrypted_bearer_token FROM mcp_connections
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(connection_id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("MCP connection not found".into()))?;

    let (url, encrypted_token) = row;
    let token = encrypted_token
        .map(|t| mcp::decrypt_token(&state.mcp_credential_key, &t))
        .transpose()
        .map_err(|e| AppError::Internal(e.into()))?;

    let client = mcp::McpClient::connect(&url, token.as_deref())
        .await
        .map_err(|e| AppError::Validation(format!("could not connect to MCP server: {e}")))?;
    let tools = client
        .list_tools()
        .await
        .map_err(|e| AppError::Validation(format!("could not list tools: {e}")))?;
    let _ = client.close().await;

    Ok(Json(
        tools
            .into_iter()
            .map(|t| McpToolResponse {
                name: t.name,
                description: t.description,
            })
            .collect(),
    ))
}

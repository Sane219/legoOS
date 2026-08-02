use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    models::{
        ExecutionNodeResponse, ExecutionResponse, WorkflowEdgeResponse, WorkflowGraphResponse,
        WorkflowNodeResponse, WorkflowResponse, WorkflowRow,
    },
    state::AppState,
    workspaces::{member_role, require_role},
};

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveNodeRequest {
    pub id: Uuid,
    pub node_type: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub position_x: f64,
    #[serde(default)]
    pub position_y: f64,
}

#[derive(Debug, Deserialize)]
pub struct SaveEdgeRequest {
    pub id: Uuid,
    pub source_node_id: Uuid,
    pub target_node_id: Uuid,
    pub condition: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveGraphRequest {
    pub nodes: Vec<SaveNodeRequest>,
    pub edges: Vec<SaveEdgeRequest>,
}

pub(crate) async fn ensure_workflow_exists(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    workflow_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM workflows WHERE id = $1 AND workspace_id = $2)",
    )
    .bind(workflow_id)
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound("workflow not found".into()))
    }
}

pub async fn create_workflow(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateWorkflowRequest>,
) -> Result<Json<WorkflowResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "workflow name must not be empty".into(),
        ));
    }

    let row = sqlx::query_as::<_, WorkflowRow>(
        "INSERT INTO workflows (workspace_id, name) VALUES ($1, $2)
         RETURNING id, name, created_at, updated_at",
    )
    .bind(workspace_id)
    .bind(name)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(row.into()))
}

pub async fn list_workflows(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<WorkflowResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let rows = sqlx::query_as::<_, WorkflowRow>(
        "SELECT id, name, created_at, updated_at FROM workflows
         WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn get_workflow(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<WorkflowGraphResponse>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let workflow = sqlx::query_as::<_, WorkflowRow>(
        "SELECT id, name, created_at, updated_at FROM workflows
         WHERE id = $1 AND workspace_id = $2",
    )
    .bind(workflow_id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("workflow not found".into()))?;

    let nodes = sqlx::query_as::<_, WorkflowNodeResponse>(
        "SELECT id, node_type, config, position_x, position_y
         FROM workflow_nodes WHERE workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let edges = sqlx::query_as::<_, WorkflowEdgeResponse>(
        "SELECT id, source_node_id, target_node_id, condition
         FROM workflow_edges WHERE workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(WorkflowGraphResponse {
        workflow: workflow.into(),
        nodes,
        edges,
    }))
}

pub async fn save_graph(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SaveGraphRequest>,
) -> Result<Json<WorkflowGraphResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    sqlx::query("DELETE FROM workflow_edges WHERE workflow_id = $1")
        .bind(workflow_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    sqlx::query("DELETE FROM workflow_nodes WHERE workflow_id = $1")
        .bind(workflow_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    for node in &body.nodes {
        sqlx::query(
            "INSERT INTO workflow_nodes (id, workflow_id, node_type, config, position_x, position_y)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(node.id)
        .bind(workflow_id)
        .bind(&node.node_type)
        .bind(&node.config)
        .bind(node.position_x)
        .bind(node.position_y)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    for edge in &body.edges {
        sqlx::query(
            "INSERT INTO workflow_edges (id, workflow_id, source_node_id, target_node_id, condition)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(edge.id)
        .bind(workflow_id)
        .bind(edge.source_node_id)
        .bind(edge.target_node_id)
        .bind(&edge.condition)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    }

    sqlx::query("UPDATE workflows SET updated_at = now() WHERE id = $1")
        .bind(workflow_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    get_workflow(
        State(state),
        AuthUser(user_id),
        Path((workspace_id, workflow_id)),
    )
    .await
}

/// Enqueues a workflow run and returns immediately; a worker process picks the job up off
/// the `queue::WORKFLOW_RUNS_STREAM` Redis stream and executes it. Poll `GET .../executions/:id`
/// (or the trace WebSocket) to observe progress.
pub async fn run_workflow(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ExecutionResponse>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let (execution_id, started_at) = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
        "INSERT INTO workflow_executions (workflow_id, status, triggered_by)
         VALUES ($1, 'pending', $2)
         RETURNING id, started_at",
    )
    .bind(workflow_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let job = queue::RunJob {
        execution_id,
        workflow_id,
    };
    let job_json = serde_json::to_string(&job).expect("RunJob always serializes");

    let mut redis = state.redis.clone();
    redis::cmd("XADD")
        .arg(queue::WORKFLOW_RUNS_STREAM)
        .arg("*")
        .arg(queue::JOB_FIELD)
        .arg(job_json)
        .query_async::<()>(&mut redis)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(ExecutionResponse {
        id: execution_id,
        workflow_id,
        status: "pending".to_string(),
        started_at,
        finished_at: None,
        nodes: Vec::new(),
    }))
}

pub async fn get_execution(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id, execution_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<ExecutionResponse>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let execution = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>)>(
        "SELECT e.id, e.status, e.started_at, e.finished_at
         FROM workflow_executions e
         JOIN workflows w ON w.id = e.workflow_id
         WHERE e.id = $1 AND e.workflow_id = $2 AND w.workspace_id = $3",
    )
    .bind(execution_id)
    .bind(workflow_id)
    .bind(workspace_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or_else(|| AppError::NotFound("execution not found".into()))?;

    let nodes = sqlx::query_as::<_, ExecutionNodeResponse>(
        "SELECT node_id, status, output, error FROM workflow_execution_nodes WHERE execution_id = $1",
    )
    .bind(execution_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(ExecutionResponse {
        id: execution.0,
        workflow_id,
        status: execution.1,
        started_at: execution.2,
        finished_at: execution.3,
        nodes,
    }))
}

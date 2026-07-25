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
    dag,
    error::AppError,
    models::{
        ExecutionNodeResponse, ExecutionResponse, WorkflowEdgeResponse, WorkflowGraphResponse,
        WorkflowNodeResponse, WorkflowResponse, WorkflowRow,
    },
    state::AppState,
    workspaces::member_role,
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

async fn ensure_workflow_exists(
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
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

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
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
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

pub async fn run_workflow(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ExecutionResponse>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;
    ensure_workflow_exists(&state.pool, workspace_id, workflow_id).await?;

    let node_rows = sqlx::query_as::<_, (Uuid, String, Value)>(
        "SELECT id, node_type, config FROM workflow_nodes WHERE workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let edge_rows = sqlx::query_as::<_, (Uuid, Uuid, Option<String>)>(
        "SELECT source_node_id, target_node_id, condition FROM workflow_edges WHERE workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    let dag_nodes: Vec<dag::Node> = node_rows
        .into_iter()
        .map(|(id, node_type, config)| dag::Node {
            id,
            node_type,
            config,
        })
        .collect();
    let dag_edges: Vec<dag::Edge> = edge_rows
        .into_iter()
        .map(|(source, target, condition)| dag::Edge {
            source,
            target,
            condition,
        })
        .collect();

    let result = dag::execute(&dag_nodes, &dag_edges);

    let status_str = match result.status {
        dag::ExecutionStatus::Succeeded => "succeeded",
        dag::ExecutionStatus::Failed => "failed",
    };

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let (execution_id, started_at, finished_at) =
        sqlx::query_as::<_, (Uuid, DateTime<Utc>, DateTime<Utc>)>(
            "INSERT INTO workflow_executions (workflow_id, status, triggered_by)
         VALUES ($1, $2, $3)
         RETURNING id, started_at, finished_at",
        )
        .bind(workflow_id)
        .bind(status_str)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    let mut node_responses = Vec::with_capacity(result.nodes.len());
    for node_result in &result.nodes {
        let node_status_str = match node_result.status {
            dag::NodeStatus::Succeeded => "succeeded",
            dag::NodeStatus::Failed => "failed",
            dag::NodeStatus::Skipped => "skipped",
        };

        sqlx::query(
            "INSERT INTO workflow_execution_nodes (execution_id, node_id, status, output, error)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(execution_id)
        .bind(node_result.node_id)
        .bind(node_status_str)
        .bind(&node_result.output)
        .bind(&node_result.error)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

        node_responses.push(ExecutionNodeResponse {
            node_id: node_result.node_id,
            status: node_status_str.to_string(),
            output: node_result.output.clone(),
            error: node_result.error.clone(),
        });
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(ExecutionResponse {
        id: execution_id,
        workflow_id,
        status: status_str.to_string(),
        started_at,
        finished_at: Some(finished_at),
        nodes: node_responses,
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

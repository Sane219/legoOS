use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id,
            email: user.email,
            created_at: user.created_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct WorkspaceWithRole {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub role: String,
}

impl From<WorkspaceWithRole> for WorkspaceResponse {
    fn from(row: WorkspaceWithRole) -> Self {
        WorkspaceResponse {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            role: row.role,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct MemberRow {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct MemberResponse {
    pub user_id: Uuid,
    pub email: String,
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

impl From<MemberRow> for MemberResponse {
    fn from(row: MemberRow) -> Self {
        MemberResponse {
            user_id: row.user_id,
            email: row.email,
            role: row.role,
            joined_at: row.joined_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct WorkflowRow {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowResponse {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<WorkflowRow> for WorkflowResponse {
    fn from(row: WorkflowRow) -> Self {
        WorkflowResponse {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WorkflowNodeResponse {
    pub id: Uuid,
    pub node_type: String,
    pub config: Value,
    pub position_x: f64,
    pub position_y: f64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WorkflowEdgeResponse {
    pub id: Uuid,
    pub source_node_id: Uuid,
    pub target_node_id: Uuid,
    pub condition: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowGraphResponse {
    #[serde(flatten)]
    pub workflow: WorkflowResponse,
    pub nodes: Vec<WorkflowNodeResponse>,
    pub edges: Vec<WorkflowEdgeResponse>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExecutionNodeResponse {
    pub node_id: Uuid,
    pub status: String,
    pub output: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionResponse {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub nodes: Vec<ExecutionNodeResponse>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct McpConnectionRow {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub has_token: bool,
    pub created_at: DateTime<Utc>,
}

/// Never carries the token (encrypted or not) back to the client — write-only from the
/// frontend's perspective, same as a password field.
#[derive(Debug, Serialize)]
pub struct McpConnectionResponse {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub has_token: bool,
    pub created_at: DateTime<Utc>,
}

impl From<McpConnectionRow> for McpConnectionResponse {
    fn from(row: McpConnectionRow) -> Self {
        McpConnectionResponse {
            id: row.id,
            name: row.name,
            url: row.url,
            has_token: row.has_token,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct ApprovalGateRow {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub node_id: Uuid,
    /// The merged upstream input the gate paused on — whatever context an approver needs
    /// to decide, captured at the moment execution reached this node.
    pub context: Option<Value>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalGateResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub node_id: Uuid,
    pub context: Option<Value>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

impl From<ApprovalGateRow> for ApprovalGateResponse {
    fn from(row: ApprovalGateRow) -> Self {
        ApprovalGateResponse {
            id: row.id,
            execution_id: row.execution_id,
            workflow_id: row.workflow_id,
            workflow_name: row.workflow_name,
            node_id: row.node_id,
            context: row.context,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

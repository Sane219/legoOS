use axum::{
    Router,
    routing::{get, post},
};

use crate::{handlers, state::AppState, workflows, workspaces};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/api/auth/register", post(handlers::register))
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/me", get(handlers::me))
        .route(
            "/api/workspaces",
            get(workspaces::list_workspaces).post(workspaces::create_workspace),
        )
        .route("/api/workspaces/{id}", get(workspaces::get_workspace))
        .route(
            "/api/workspaces/{id}/members",
            get(workspaces::list_members).post(workspaces::add_member),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows",
            get(workflows::list_workflows).post(workflows::create_workflow),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}",
            get(workflows::get_workflow).put(workflows::save_graph),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions",
            post(workflows::run_workflow),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}",
            get(workflows::get_execution),
        )
        .with_state(state)
}

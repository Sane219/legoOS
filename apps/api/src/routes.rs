use axum::{
    Router,
    routing::{get, post},
};

use crate::{
    analytics, approvals, documents, handlers, mcp_connections, metrics, schedules,
    state::AppState, trace, workflows, workspaces,
};

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/metrics", get(metrics::render))
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
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}/trace",
            get(trace::execution_trace),
        )
        .route(
            "/api/workspaces/{workspace_id}/mcp-connections",
            get(mcp_connections::list_connections).post(mcp_connections::create_connection),
        )
        .route(
            "/api/workspaces/{workspace_id}/mcp-connections/{connection_id}",
            axum::routing::delete(mcp_connections::delete_connection),
        )
        .route(
            "/api/workspaces/{workspace_id}/mcp-connections/{connection_id}/test",
            post(mcp_connections::test_connection),
        )
        .route(
            "/api/workspaces/{workspace_id}/approvals",
            get(approvals::list_approvals),
        )
        .route(
            "/api/workspaces/{workspace_id}/approvals/{gate_id}/approve",
            post(approvals::approve),
        )
        .route(
            "/api/workspaces/{workspace_id}/approvals/{gate_id}/reject",
            post(approvals::reject),
        )
        .route(
            "/api/workspaces/{workspace_id}/documents",
            get(documents::list_documents).post(documents::create_document),
        )
        .route(
            "/api/workspaces/{workspace_id}/documents/{document_id}",
            axum::routing::delete(documents::delete_document),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules",
            get(schedules::list_schedules).post(schedules::create_schedule),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules/{schedule_id}",
            axum::routing::patch(schedules::update_schedule)
                .delete(schedules::delete_schedule),
        )
        .route(
            "/api/workspaces/{workspace_id}/workflows/{workflow_id}/analytics",
            get(analytics::workflow_analytics),
        )
        .layer(axum::middleware::from_fn(metrics::track_http_metrics))
        .with_state(state)
}

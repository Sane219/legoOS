mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    add_member, app, authed_json_request, authed_request, create_workspace, json_body, register,
    run_execution_inline,
};

async fn create_workflow_id(app: axum::Router, token: &str, workspace_id: &str) -> String {
    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows"),
            token,
            json!({ "name": "Workflow" }),
        ))
        .await
        .unwrap();
    json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// An `evaluate` node's rule-mode score should show up in the analytics rollup for the
/// execution it ran in, aggregated straight from `workflow_execution_nodes.output`.
#[sqlx::test]
async fn analytics_reports_eval_score_for_a_finished_execution(pool: PgPool) {
    let app = app(pool.clone()).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let input_node = Uuid::new_v4();
    let evaluate_node = Uuid::new_v4();

    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [
                    { "id": input_node, "node_type": "input", "config": { "value": { "response": "the cat sat on the mat" } }, "position_x": 0.0, "position_y": 0.0 },
                    { "id": evaluate_node, "node_type": "evaluate", "config": {
                        "mode": "rule",
                        "rules": [{ "type": "contains", "value": "cat" }],
                    }, "position_x": 100.0, "position_y": 0.0 },
                ],
                "edges": [
                    { "id": Uuid::new_v4(), "source_node_id": input_node, "target_node_id": evaluate_node, "condition": null },
                ],
            }),
        ))
        .await
        .unwrap();

    let run_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions"),
            &token,
        ))
        .await
        .unwrap();
    let run_body = json_body(run_response).await;
    let execution_id = Uuid::parse_str(run_body["id"].as_str().unwrap()).unwrap();
    let workflow_uuid = Uuid::parse_str(&workflow_id).unwrap();
    run_execution_inline(&pool, execution_id, workflow_uuid).await;

    let analytics_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/analytics"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(analytics_response.status(), StatusCode::OK);
    let body = json_body(analytics_response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["execution_id"], execution_id.to_string());
    assert_eq!(rows[0]["status"], "succeeded");
    assert_eq!(rows[0]["avg_eval_score"], 1.0);
    assert_eq!(rows[0]["total_cost_usd"], 0.0);
}

#[sqlx::test]
async fn analytics_forbidden_for_non_member(pool: PgPool) {
    let app = app(pool).await;
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &owner_token, &workspace_id).await;

    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/analytics"),
            &stranger_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn analytics_is_visible_to_any_member_not_just_owner(pool: PgPool) {
    let app = app(pool).await;
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let member_token = register(app.clone(), "member@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;
    add_member(
        app.clone(),
        &owner_token,
        &workspace_id,
        "member@example.com",
    )
    .await;
    let workflow_id = create_workflow_id(app.clone(), &owner_token, &workspace_id).await;

    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/analytics"),
            &member_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

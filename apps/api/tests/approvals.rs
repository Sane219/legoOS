mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    app, authed_json_request, authed_request, create_workspace, json_body, register,
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

/// Builds input -> approval -> transform and returns (workflow_id, gate_node_id).
async fn save_gated_graph(
    app: axum::Router,
    token: &str,
    workspace_id: &str,
    workflow_id: &str,
) -> Uuid {
    let input_node = Uuid::new_v4();
    let gate_node = Uuid::new_v4();
    let downstream_node = Uuid::new_v4();

    app.oneshot(authed_json_request(
        "PUT",
        &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
        token,
        json!({
            "nodes": [
                { "id": input_node, "node_type": "input", "config": { "value": { "amount": 100 } }, "position_x": 0.0, "position_y": 0.0 },
                { "id": gate_node, "node_type": "approval", "config": {}, "position_x": 100.0, "position_y": 0.0 },
                { "id": downstream_node, "node_type": "transform", "config": { "merge": { "paid": true } }, "position_x": 200.0, "position_y": 0.0 },
            ],
            "edges": [
                { "id": Uuid::new_v4(), "source_node_id": input_node, "target_node_id": gate_node, "condition": null },
                { "id": Uuid::new_v4(), "source_node_id": gate_node, "target_node_id": downstream_node, "condition": null },
            ],
        }),
    ))
    .await
    .unwrap();

    gate_node
}

#[sqlx::test]
async fn run_pauses_at_approval_gate_and_lists_in_inbox(pool: PgPool) {
    let app = app(pool.clone()).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;
    save_gated_graph(app.clone(), &token, &workspace_id, &workflow_id).await;

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

    let get_response = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!(
                "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}"
            ),
            &token,
        ))
        .await
        .unwrap();
    let body = json_body(get_response).await;
    assert_eq!(body["status"], "waiting");

    let inbox_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/approvals"),
            &token,
        ))
        .await
        .unwrap();
    let inbox = json_body(inbox_response).await;
    let gates = inbox.as_array().unwrap();
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0]["execution_id"], execution_id.to_string());
    assert_eq!(gates[0]["status"], "pending");
    assert_eq!(gates[0]["context"], json!({ "amount": 100 }));
}

#[sqlx::test]
async fn approving_resumes_the_workflow_to_completion(pool: PgPool) {
    let app = app(pool.clone()).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;
    let gate_node = save_gated_graph(app.clone(), &token, &workspace_id, &workflow_id).await;

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

    let inbox = json_body(
        app.clone()
            .oneshot(authed_request(
                "GET",
                &format!("/api/workspaces/{workspace_id}/approvals"),
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    let gate_id = inbox.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let approve_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/approvals/{gate_id}/approve"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);

    // The API enqueued a resume job on the real stream; drive it the same way the test
    // harness drives the initial run, since a live worker isn't running in this process.
    run_execution_inline(&pool, execution_id, workflow_uuid).await;

    let get_response = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!(
                "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}"
            ),
            &token,
        ))
        .await
        .unwrap();
    let body = json_body(get_response).await;
    assert_eq!(body["status"], "succeeded");
    let nodes = body["nodes"].as_array().unwrap();
    let gate_result = nodes
        .iter()
        .find(|n| n["node_id"] == gate_node.to_string())
        .unwrap();
    assert_eq!(gate_result["status"], "succeeded");

    let inbox_after = json_body(
        app.oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/approvals"),
            &token,
        ))
        .await
        .unwrap(),
    )
    .await;
    assert!(inbox_after.as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn rejecting_fails_the_gate_without_running_downstream(pool: PgPool) {
    let app = app(pool.clone()).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;
    save_gated_graph(app.clone(), &token, &workspace_id, &workflow_id).await;

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

    let inbox = json_body(
        app.clone()
            .oneshot(authed_request(
                "GET",
                &format!("/api/workspaces/{workspace_id}/approvals"),
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    let gate_id = inbox.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let reject_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/approvals/{gate_id}/reject"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(reject_response.status(), StatusCode::OK);

    run_execution_inline(&pool, execution_id, workflow_uuid).await;

    let get_response = app
        .oneshot(authed_request(
            "GET",
            &format!(
                "/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}"
            ),
            &token,
        ))
        .await
        .unwrap();
    let body = json_body(get_response).await;
    assert_eq!(body["status"], "failed");
}

#[sqlx::test]
async fn approve_404s_for_non_member(pool: PgPool) {
    let app = app(pool.clone()).await;
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;

    let response = app
        .oneshot(authed_request(
            "POST",
            &format!(
                "/api/workspaces/{workspace_id}/approvals/{}/approve",
                Uuid::new_v4()
            ),
            &stranger_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{app, authed_json_request, authed_request, create_workspace, json_body, register};

#[sqlx::test]
async fn create_workflow_succeeds(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows"),
            &token,
            json!({ "name": "My Workflow" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["name"], "My Workflow");
}

#[sqlx::test]
async fn create_workflow_404_for_non_member(pool: PgPool) {
    let app = app(pool);
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows"),
            &stranger_token,
            json!({ "name": "Intruder Workflow" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn list_workflows_is_scoped_to_workspace(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_a = create_workspace(app.clone(), &token, "Workspace A").await;
    let workspace_b = create_workspace(app.clone(), &token, "Workspace B").await;

    app.clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_a}/workflows"),
            &token,
            json!({ "name": "Only in A" }),
        ))
        .await
        .unwrap();

    let response_a = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_a}/workflows"),
            &token,
        ))
        .await
        .unwrap();
    let workflows_a = json_body(response_a).await;
    assert_eq!(workflows_a.as_array().unwrap().len(), 1);

    let response_b = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_b}/workflows"),
            &token,
        ))
        .await
        .unwrap();
    let workflows_b = json_body(response_b).await;
    assert_eq!(workflows_b.as_array().unwrap().len(), 0);
}

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

#[sqlx::test]
async fn save_graph_then_get_returns_nodes_and_edges(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let node_a = Uuid::new_v4();
    let node_b = Uuid::new_v4();
    let edge_id = Uuid::new_v4();

    let save_response = app
        .clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [
                    { "id": node_a, "node_type": "input", "config": { "value": { "x": 1 } }, "position_x": 0.0, "position_y": 0.0 },
                    { "id": node_b, "node_type": "transform", "config": { "merge": { "y": 2 } }, "position_x": 100.0, "position_y": 0.0 },
                ],
                "edges": [
                    { "id": edge_id, "source_node_id": node_a, "target_node_id": node_b, "condition": null },
                ],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(save_response.status(), StatusCode::OK);

    let get_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let body = json_body(get_response).await;
    assert_eq!(body["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn save_graph_replaces_previous_graph(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let first_node = Uuid::new_v4();
    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [{ "id": first_node, "node_type": "input", "config": {}, "position_x": 0.0, "position_y": 0.0 }],
                "edges": [],
            }),
        ))
        .await
        .unwrap();

    let second_node = Uuid::new_v4();
    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [{ "id": second_node, "node_type": "input", "config": {}, "position_x": 0.0, "position_y": 0.0 }],
                "edges": [],
            }),
        ))
        .await
        .unwrap();

    let get_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
        ))
        .await
        .unwrap();
    let body = json_body(get_response).await;
    let nodes = body["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["id"], second_node.to_string());
}

#[sqlx::test]
async fn run_workflow_executes_linear_chain(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let input_node = Uuid::new_v4();
    let transform_node = Uuid::new_v4();

    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [
                    { "id": input_node, "node_type": "input", "config": { "value": { "x": 1 } }, "position_x": 0.0, "position_y": 0.0 },
                    { "id": transform_node, "node_type": "transform", "config": { "merge": { "y": 2 } }, "position_x": 100.0, "position_y": 0.0 },
                ],
                "edges": [
                    { "id": Uuid::new_v4(), "source_node_id": input_node, "target_node_id": transform_node, "condition": null },
                ],
            }),
        ))
        .await
        .unwrap();

    let run_response = app
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions"),
            &token,
        ))
        .await
        .unwrap();

    assert_eq!(run_response.status(), StatusCode::OK);
    let body = json_body(run_response).await;
    assert_eq!(body["status"], "succeeded");

    let nodes = body["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    let transform_result = nodes
        .iter()
        .find(|n| n["node_id"] == transform_node.to_string())
        .unwrap();
    assert_eq!(transform_result["status"], "succeeded");
    assert_eq!(transform_result["output"], json!({ "x": 1, "y": 2 }));
}

#[sqlx::test]
async fn run_workflow_records_branch_skip(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let input_node = Uuid::new_v4();
    let cond_node = Uuid::new_v4();
    let on_true = Uuid::new_v4();
    let on_false = Uuid::new_v4();

    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [
                    { "id": input_node, "node_type": "input", "config": { "value": { "flag": true } }, "position_x": 0.0, "position_y": 0.0 },
                    { "id": cond_node, "node_type": "condition", "config": { "field": "flag", "equals": true }, "position_x": 100.0, "position_y": 0.0 },
                    { "id": on_true, "node_type": "transform", "config": {}, "position_x": 200.0, "position_y": -50.0 },
                    { "id": on_false, "node_type": "transform", "config": {}, "position_x": 200.0, "position_y": 50.0 },
                ],
                "edges": [
                    { "id": Uuid::new_v4(), "source_node_id": input_node, "target_node_id": cond_node, "condition": null },
                    { "id": Uuid::new_v4(), "source_node_id": cond_node, "target_node_id": on_true, "condition": "true" },
                    { "id": Uuid::new_v4(), "source_node_id": cond_node, "target_node_id": on_false, "condition": "false" },
                ],
            }),
        ))
        .await
        .unwrap();

    let run_response = app
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions"),
            &token,
        ))
        .await
        .unwrap();

    let body = json_body(run_response).await;
    assert_eq!(body["status"], "succeeded");
    let nodes = body["nodes"].as_array().unwrap();

    let true_result = nodes
        .iter()
        .find(|n| n["node_id"] == on_true.to_string())
        .unwrap();
    let false_result = nodes
        .iter()
        .find(|n| n["node_id"] == on_false.to_string())
        .unwrap();
    assert_eq!(true_result["status"], "succeeded");
    assert_eq!(false_result["status"], "skipped");
}

#[sqlx::test]
async fn get_execution_returns_persisted_result(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let input_node = Uuid::new_v4();
    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [{ "id": input_node, "node_type": "input", "config": { "value": 42 }, "position_x": 0.0, "position_y": 0.0 }],
                "edges": [],
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
    let execution_id = run_body["id"].as_str().unwrap().to_string();

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

    assert_eq!(get_response.status(), StatusCode::OK);
    let body = json_body(get_response).await;
    assert_eq!(body["id"], execution_id);
    assert_eq!(body["status"], "succeeded");
    assert_eq!(body["nodes"].as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn run_workflow_404_for_non_member(pool: PgPool) {
    let app = app(pool);
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &owner_token, &workspace_id).await;

    let response = app
        .oneshot(authed_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions"),
            &stranger_token,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

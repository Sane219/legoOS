mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{
    add_member, app, authed_json_request, authed_request, create_workspace, json_body, register,
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

#[sqlx::test]
async fn create_list_update_and_delete_schedule(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules"),
            &token,
            json!({ "cron_expression": "0 9 * * *" }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let created = json_body(create_response).await;
    assert_eq!(created["enabled"], true);
    assert!(created["next_run_at"].is_string());
    let schedule_id = created["id"].as_str().unwrap().to_string();

    let list_response = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules"),
            &token,
        ))
        .await
        .unwrap();
    let list = json_body(list_response).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let disable_response = app
        .clone()
        .oneshot(authed_json_request(
            "PATCH",
            &format!(
                "/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules/{schedule_id}"
            ),
            &token,
            json!({ "enabled": false }),
        ))
        .await
        .unwrap();
    assert_eq!(disable_response.status(), StatusCode::OK);
    assert_eq!(json_body(disable_response).await["enabled"], false);

    let delete_response = app
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!(
                "/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules/{schedule_id}"
            ),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_after_delete = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules"),
            &token,
        ))
        .await
        .unwrap();
    assert!(
        json_body(list_after_delete)
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn create_schedule_rejects_invalid_cron(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;
    let workflow_id = create_workflow_id(app.clone(), &token, &workspace_id).await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules"),
            &token,
            json!({ "cron_expression": "not a cron" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn create_schedule_forbidden_for_member(pool: PgPool) {
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
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}/schedules"),
            &member_token,
            json!({ "cron_expression": "0 9 * * *" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

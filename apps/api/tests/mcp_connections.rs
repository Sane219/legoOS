mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{
    add_member, app, authed_json_request, authed_request, create_workspace, json_body, register,
};

#[sqlx::test]
async fn create_list_and_delete_connection(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
            json!({ "name": "weather", "url": "http://localhost:9999/mcp", "bearer_token": "secret" }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let created = json_body(create_response).await;
    assert_eq!(created["name"], "weather");
    assert_eq!(created["has_token"], true);
    // The token itself, encrypted or not, is never returned.
    assert!(created.get("bearer_token").is_none());
    assert!(created.get("encrypted_bearer_token").is_none());
    let connection_id = created["id"].as_str().unwrap().to_string();

    let list_response = app
        .clone()
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
        ))
        .await
        .unwrap();
    let list = json_body(list_response).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let delete_response = app
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/workspaces/{workspace_id}/mcp-connections/{connection_id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_after_delete = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
        ))
        .await
        .unwrap();
    let list_after_delete = json_body(list_after_delete).await;
    assert_eq!(list_after_delete.as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn create_connection_rejects_duplicate_name(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let body = json!({ "name": "weather", "url": "http://localhost:9999/mcp" });

    let first = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
            body.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
            body,
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn create_connection_rejects_non_http_url(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &token,
            json!({ "name": "evil", "url": "file:///etc/passwd" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn mcp_connections_404_for_non_member(pool: PgPool) {
    let app = app(pool).await;
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;

    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &stranger_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn create_connection_forbidden_for_member(pool: PgPool) {
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

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &member_token,
            json!({ "name": "weather", "url": "http://localhost:9999/mcp" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn delete_connection_forbidden_for_member(pool: PgPool) {
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

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &owner_token,
            json!({ "name": "weather", "url": "http://localhost:9999/mcp" }),
        ))
        .await
        .unwrap();
    let connection_id = json_body(create_response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = app
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/workspaces/{workspace_id}/mcp-connections/{connection_id}"),
            &member_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn list_connections_allowed_for_member(pool: PgPool) {
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

    // A member can view connections (a read action) even though they can't create/delete
    // one (a configuration action).
    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/mcp-connections"),
            &member_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

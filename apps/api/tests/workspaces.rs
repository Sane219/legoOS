mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{app, authed_json_request, authed_request, json_body, register};

#[sqlx::test]
async fn create_workspace_succeeds(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token,
            json!({ "name": "Acme" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["name"], "Acme");
    assert_eq!(body["role"], "owner");
}

#[sqlx::test]
async fn create_workspace_rejects_empty_name(pool: PgPool) {
    let app = app(pool);
    let token = register(app.clone(), "owner@example.com", "hunter22").await;

    let response = app
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token,
            json!({ "name": "   " }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn list_workspaces_returns_only_member_workspaces(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;
    let token_b = register(app.clone(), "bob@example.com", "hunter22").await;

    app.clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    app.clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_b,
            json!({ "name": "Bob's workspace" }),
        ))
        .await
        .unwrap();

    let response = app
        .oneshot(authed_request("GET", "/api/workspaces", &token_a))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let workspaces = body.as_array().unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0]["name"], "Alice's workspace");
}

#[sqlx::test]
async fn get_workspace_404_for_non_member(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;
    let token_b = register(app.clone(), "bob@example.com", "hunter22").await;

    let created = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    let workspace_id = json_body(created).await["id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}"),
            &token_b,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn add_member_succeeds_when_owner_and_appears_in_member_list(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;
    let _token_b = register(app.clone(), "bob@example.com", "hunter22").await;

    let created = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    let workspace_id = json_body(created).await["id"].as_str().unwrap().to_string();

    let add_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
            json!({ "email": "bob@example.com" }),
        ))
        .await
        .unwrap();
    assert_eq!(add_response.status(), StatusCode::OK);
    let added = json_body(add_response).await;
    assert_eq!(added["role"], "member");

    let members_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
        ))
        .await
        .unwrap();
    assert_eq!(members_response.status(), StatusCode::OK);
    let members = json_body(members_response).await;
    let members = members.as_array().unwrap();
    assert_eq!(members.len(), 2);
}

#[sqlx::test]
async fn add_member_forbidden_when_not_owner(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;
    let token_b = register(app.clone(), "bob@example.com", "hunter22").await;
    let _token_c = register(app.clone(), "carol@example.com", "hunter22").await;

    let created = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    let workspace_id = json_body(created).await["id"].as_str().unwrap().to_string();

    app.clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
            json!({ "email": "bob@example.com" }),
        ))
        .await
        .unwrap();

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_b,
            json!({ "email": "carol@example.com" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn add_member_conflict_when_already_a_member(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;
    let _token_b = register(app.clone(), "bob@example.com", "hunter22").await;

    let created = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    let workspace_id = json_body(created).await["id"].as_str().unwrap().to_string();

    app.clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
            json!({ "email": "bob@example.com" }),
        ))
        .await
        .unwrap();

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
            json!({ "email": "bob@example.com" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[sqlx::test]
async fn add_member_validation_when_email_unknown(pool: PgPool) {
    let app = app(pool);
    let token_a = register(app.clone(), "alice@example.com", "hunter22").await;

    let created = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            &token_a,
            json!({ "name": "Alice's workspace" }),
        ))
        .await
        .unwrap();
    let workspace_id = json_body(created).await["id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/members"),
            &token_a,
            json!({ "email": "nobody@example.com" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

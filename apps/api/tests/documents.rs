mod common;

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{
    add_member, app, authed_json_request, authed_request, create_workspace, json_body, register,
};

/// Polls the document list a few times, since ingestion runs on a spawned background task
/// rather than being awaited by the create response.
async fn wait_for_status(
    app: axum::Router,
    token: &str,
    workspace_id: &str,
    document_id: &str,
    want_status: &str,
) -> serde_json::Value {
    for _ in 0..50 {
        let response = app
            .clone()
            .oneshot(authed_request(
                "GET",
                &format!("/api/workspaces/{workspace_id}/documents"),
                token,
            ))
            .await
            .unwrap();
        let list = json_body(response).await;
        if let Some(doc) = list
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == document_id && d["status"] == want_status)
        {
            return doc.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("document {document_id} never reached status {want_status}");
}

#[sqlx::test]
async fn uploading_a_document_ingests_it_to_ready(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/documents"),
            &token,
            json!({ "name": "notes.txt", "content": "the sky is blue and the grass is green" }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let created = json_body(create_response).await;
    assert_eq!(created["status"], "pending");
    let document_id = created["id"].as_str().unwrap().to_string();

    let ready = wait_for_status(app, &token, &workspace_id, &document_id, "ready").await;
    assert_eq!(ready["error"], serde_json::Value::Null);
}

#[sqlx::test]
async fn deleting_a_document_removes_it_from_the_list(pool: PgPool) {
    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/documents"),
            &token,
            json!({ "name": "notes.txt", "content": "short document" }),
        ))
        .await
        .unwrap();
    let document_id = json_body(create_response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let delete_response = app
        .clone()
        .oneshot(authed_request(
            "DELETE",
            &format!("/api/workspaces/{workspace_id}/documents/{document_id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/documents"),
            &token,
        ))
        .await
        .unwrap();
    let list = json_body(list_response).await;
    assert!(list.as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn create_document_forbidden_for_member(pool: PgPool) {
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
            &format!("/api/workspaces/{workspace_id}/documents"),
            &member_token,
            json!({ "name": "notes.txt", "content": "hello" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn documents_404_for_non_member(pool: PgPool) {
    let app = app(pool).await;
    let owner_token = register(app.clone(), "owner@example.com", "hunter22").await;
    let stranger_token = register(app.clone(), "stranger@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &owner_token, "Acme").await;

    let response = app
        .oneshot(authed_request(
            "GET",
            &format!("/api/workspaces/{workspace_id}/documents"),
            &stranger_token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

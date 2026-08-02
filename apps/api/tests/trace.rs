mod common;

use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tower::ServiceExt;
use uuid::Uuid;

use common::{authed_json_request, authed_request, create_workspace, json_body, register};

/// Binds the router to a real port so a WebSocket client can connect (WS upgrades need a
/// live TCP connection; `tower::ServiceExt::oneshot` can't hijack the transport for one).
async fn spawn(app: axum::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("ws://{addr}")
}

#[sqlx::test]
async fn execution_trace_streams_node_results_then_final(pool: PgPool) {
    let app = common::app(pool.clone()).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;
    let workspace_id = create_workspace(app.clone(), &token, "Acme").await;

    let create_response = app
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/workspaces/{workspace_id}/workflows"),
            &token,
            json!({ "name": "Workflow" }),
        ))
        .await
        .unwrap();
    let workflow_id = json_body(create_response).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    let input_node = Uuid::new_v4();
    app.clone()
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/workspaces/{workspace_id}/workflows/{workflow_id}"),
            &token,
            json!({
                "nodes": [{ "id": input_node, "node_type": "input", "config": { "value": 1 }, "position_x": 0.0, "position_y": 0.0 }],
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
    assert_eq!(run_response.status(), StatusCode::OK);
    let run_body = json_body(run_response).await;
    let execution_id = Uuid::parse_str(run_body["id"].as_str().unwrap()).unwrap();
    let workflow_uuid = Uuid::parse_str(&workflow_id).unwrap();

    let base_url = spawn(app).await;
    let url = format!(
        "{base_url}/api/workspaces/{workspace_id}/workflows/{workflow_id}/executions/{execution_id}/trace"
    );
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());

    let (mut socket, response) = connect_async(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    // Run the job only after the socket is subscribed, proving events arrive live rather
    // than only via the DB-backed replay.
    common::run_execution_inline(&pool, execution_id, workflow_uuid).await;

    let mut saw_node_result = false;
    let mut saw_final = false;
    for _ in 0..10 {
        let Some(Ok(msg)) = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a trace event")
        else {
            break;
        };
        let text = msg.into_text().unwrap();
        let event: Value = serde_json::from_str(&text).unwrap();
        match event["type"].as_str().unwrap() {
            "node_result" => saw_node_result = true,
            "final" => {
                saw_final = true;
                assert_eq!(event["status"], "succeeded");
                break;
            }
            other => panic!("unexpected event type: {other}"),
        }
    }

    assert!(saw_node_result, "expected at least one node_result event");
    assert!(saw_final, "expected a final event");

    let _ = socket.close(None).await;
}

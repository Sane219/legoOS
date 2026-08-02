// Shared across multiple test binaries; not every binary that includes this module uses
// every helper, which would otherwise warn per-binary.
#![allow(dead_code)]

use api::{routes, state::AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use redis::aio::ConnectionManager;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub const JWT_SECRET: &str = "test-secret";

pub fn redis_client() -> redis::Client {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    redis::Client::open(redis_url.as_str()).expect("invalid REDIS_URL")
}

pub async fn redis_conn() -> ConnectionManager {
    ConnectionManager::new(redis_client())
        .await
        .expect("failed to connect to redis for tests")
}

pub async fn app(pool: PgPool) -> axum::Router {
    let redis = redis_conn().await;

    routes::build(AppState {
        pool,
        jwt_secret: JWT_SECRET.to_string(),
        redis,
        redis_client: redis_client(),
    })
}

pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

pub fn register_request(email: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .unwrap()
}

pub fn authed_json_request(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn authed_request(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// Registers a fresh user and returns their JWT.
pub async fn register(app: axum::Router, email: &str, password: &str) -> String {
    let response = app
        .oneshot(register_request(email, password))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Runs an already-enqueued execution to completion in-process, bypassing the Redis
/// stream (each `#[sqlx::test]` shares one Redis instance, so reading the real stream
/// would race with other tests' jobs). Mirrors exactly what the worker binary does for
/// one job, giving genuine coverage of the enqueue -> execute -> persist path.
pub async fn run_execution_inline(pool: &PgPool, execution_id: Uuid, workflow_id: Uuid) {
    let mut redis = redis_conn().await;
    let job = queue::RunJob {
        execution_id,
        workflow_id,
    };
    worker::run_job(pool, &mut redis, &job, None)
        .await
        .expect("worker::run_job failed");
}

/// Creates a workspace owned by `token`'s user and returns its id.
pub async fn create_workspace(app: axum::Router, token: &str, name: &str) -> String {
    let response = app
        .oneshot(authed_json_request(
            "POST",
            "/api/workspaces",
            token,
            json!({ "name": name }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

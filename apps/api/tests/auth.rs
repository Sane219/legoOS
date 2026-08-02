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

const JWT_SECRET: &str = "test-secret";
const MCP_CREDENTIAL_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn app(pool: PgPool) -> axum::Router {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(redis_url.as_str()).expect("invalid REDIS_URL");
    let redis = ConnectionManager::new(client.clone())
        .await
        .expect("failed to connect to redis for tests");
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string());
    let rag_client = rag::RagClient::connect(&qdrant_url).expect("invalid QDRANT_URL");

    routes::build(AppState {
        pool,
        jwt_secret: JWT_SECRET.to_string(),
        redis,
        redis_client: client,
        mcp_credential_key: MCP_CREDENTIAL_KEY.to_string(),
        rag_client,
        embedding_provider: None,
    })
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn register_request(email: &str, password: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .unwrap()
}

#[sqlx::test]
async fn register_succeeds(pool: PgPool) {
    let app = app(pool).await;

    let response = app
        .oneshot(register_request("alice@example.com", "hunter22"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.get("token").and_then(Value::as_str).is_some());
}

#[sqlx::test]
async fn register_duplicate_email_fails(pool: PgPool) {
    let app = app(pool).await;

    let first = app
        .clone()
        .oneshot(register_request("bob@example.com", "hunter22"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(register_request("bob@example.com", "hunter22"))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[sqlx::test]
async fn login_succeeds(pool: PgPool) {
    let app = app(pool).await;

    app.clone()
        .oneshot(register_request("carol@example.com", "hunter22"))
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "carol@example.com", "password": "hunter22" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.get("token").and_then(Value::as_str).is_some());
}

#[sqlx::test]
async fn login_wrong_password_fails(pool: PgPool) {
    let app = app(pool).await;

    app.clone()
        .oneshot(register_request("dave@example.com", "hunter22"))
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "dave@example.com", "password": "wrong-password" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn me_with_valid_token_succeeds(pool: PgPool) {
    let app = app(pool).await;

    let register_response = app
        .clone()
        .oneshot(register_request("erin@example.com", "hunter22"))
        .await
        .unwrap();
    let token = json_body(register_response).await["token"]
        .as_str()
        .unwrap()
        .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["email"], "erin@example.com");
}

#[sqlx::test]
async fn me_without_token_returns_401(pool: PgPool) {
    let app = app(pool).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

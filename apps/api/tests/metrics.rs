mod common;

use axum::http::StatusCode;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{app, authed_request, register};

/// `/metrics` should expose real Prometheus text once the recorder is installed and at
/// least one request has gone through the tracking middleware.
#[sqlx::test]
async fn metrics_endpoint_reports_http_request_counts(pool: PgPool) {
    api::metrics::install();

    let app = app(pool).await;
    let token = register(app.clone(), "owner@example.com", "hunter22").await;

    app.clone()
        .oneshot(authed_request("GET", "/api/auth/me", &token))
        .await
        .unwrap();

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("http_requests_total"));
    assert!(body.contains("http_request_duration_seconds"));
}

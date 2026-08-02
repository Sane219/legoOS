use axum::{extract::Request, middleware::Next, response::Response};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use std::time::Instant;

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Installs the process-wide Prometheus recorder (idempotent — safe to call from more
/// than one test in the same process). Executor's node metrics have no recorder of their
/// own; they record into whichever one is installed here.
pub fn install() {
    PROMETHEUS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install Prometheus recorder")
    });
}

pub async fn render() -> String {
    PROMETHEUS_HANDLE
        .get()
        .map(|h| h.render())
        .unwrap_or_default()
}

/// Records a request-count and duration histogram per (method, route pattern, status).
/// Uses the matched route pattern (`req.uri()` before axum resolves path params to a
/// pattern isn't available here, so we fall back to the raw path — cardinality is bounded
/// in practice by this API's small, fixed route set).
pub async fn track_http_metrics(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let started_at = Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status.clone(),
    )
    .record(started_at.elapsed().as_secs_f64());
    metrics::counter!(
        "http_requests_total",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .increment(1);

    response
}

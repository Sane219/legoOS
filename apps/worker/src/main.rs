use tracing_subscriber::EnvFilter;

// ponytail: this process currently idles — there's no queue for it to consume from yet.
// v1's DAG executor runs in-process inside `apps/api` (see the `executor` crate). Phase 2
// ("Introduce the queue and move node execution from in-process to worker processes") is
// what wires this binary up to actually pull and run workflow node tasks.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tracing::info!("worker starting (no queue configured yet — idling)");

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");

    tracing::info!("worker shutting down");
}

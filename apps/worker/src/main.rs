use anyhow::Context;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use worker::{ensure_group, process_entry, read_new, reclaim_stuck};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("failed to connect to database")?;

    let client = redis::Client::open(redis_url.as_str()).context("invalid REDIS_URL")?;
    let mut redis = ConnectionManager::new(client)
        .await
        .context("failed to connect to redis")?;

    ensure_group(&mut redis).await?;

    let provider: Option<Arc<dyn llm::LlmProvider>> = match llm::provider_from_env() {
        Ok(p) => Some(Arc::from(p)),
        Err(e) => {
            tracing::warn!(error = %e, "no LLM provider configured; agent nodes will fail");
            None
        }
    };

    let consumer = format!("worker-{}", std::process::id());
    tracing::info!(consumer = %consumer, "worker started, waiting for workflow runs");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("worker shutting down");
                break;
            }
            _ = tick(&pool, &mut redis, &consumer, provider.as_ref()) => {}
        }
    }

    Ok(())
}

async fn tick(
    pool: &PgPool,
    redis: &mut ConnectionManager,
    consumer: &str,
    provider: Option<&Arc<dyn llm::LlmProvider>>,
) {
    if let Err(e) = reclaim_stuck(pool, redis, consumer, provider).await {
        tracing::warn!(error = %e, "reclaim pass failed");
    }

    match read_new(redis, consumer, 2000).await {
        Ok(entries) => {
            for (entry_id, job) in entries {
                process_entry(pool, redis, &entry_id, job, provider).await;
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "xreadgroup failed");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

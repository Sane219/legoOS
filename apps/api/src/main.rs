use anyhow::Context;
use api::{routes, state::AppState};
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    api::metrics::install();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let jwt_secret = std::env::var("JWT_SECRET").context("JWT_SECRET must be set")?;
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let mcp_credential_key =
        std::env::var("MCP_CREDENTIAL_KEY").context("MCP_CREDENTIAL_KEY must be set")?;
    if mcp_credential_key.len() != 64 {
        anyhow::bail!("MCP_CREDENTIAL_KEY must be 64 hex characters (32 bytes)");
    }
    let qdrant_url =
        std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .context("failed to connect to database")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run migrations")?;

    let redis_client = redis::Client::open(redis_url.as_str()).context("invalid REDIS_URL")?;
    let redis = ConnectionManager::new(redis_client.clone())
        .await
        .context("failed to connect to redis")?;

    let rag_client = rag::RagClient::connect(&qdrant_url).context("invalid QDRANT_URL")?;

    let embedding_provider: Option<Arc<dyn llm::EmbeddingProvider>> =
        match llm::embedding_provider_from_env() {
            Ok(p) => Some(Arc::from(p)),
            Err(e) => {
                tracing::warn!(error = %e, "no embedding provider configured; document ingestion will fail");
                None
            }
        };

    let state = AppState {
        pool,
        jwt_secret,
        redis,
        redis_client,
        mcp_credential_key,
        rag_client,
        embedding_provider,
    };
    let app = routes::build(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis),
                ),
        )
        // ponytail: wide open for local/dev use (auth is a bearer token, not cookies, so this
        // isn't a CSRF hole yet); scope to known frontend origins during the Phase 5 security pass.
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:8080";
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!("listening on {addr}");
    axum::serve(listener, app).await.context("server error")?;

    Ok(())
}

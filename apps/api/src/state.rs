use redis::aio::ConnectionManager;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: String,
    /// Multiplexed connection for regular commands (XADD, etc).
    pub redis: ConnectionManager,
    /// Cloned per WebSocket connection to open a dedicated pub/sub subscription;
    /// `redis` above can't be reused for that since it isn't a pub/sub connection.
    pub redis_client: redis::Client,
    /// AES-256-GCM key (64 hex chars) used to encrypt MCP server bearer tokens at rest.
    pub mcp_credential_key: String,
}

use redis::aio::ConnectionManager;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MCP_CREDENTIAL_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WeatherRequest {
    city: String,
}

#[derive(Debug, Clone)]
struct WeatherServer {
    tool_router: ToolRouter<Self>,
}

impl WeatherServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl WeatherServer {
    #[tool(description = "Looks up the weather for a city")]
    fn weather(&self, Parameters(WeatherRequest { city }): Parameters<WeatherRequest>) -> String {
        format!("sunny in {city}")
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WeatherServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}

async fn redis_conn() -> ConnectionManager {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(redis_url.as_str()).expect("invalid REDIS_URL");
    ConnectionManager::new(client)
        .await
        .expect("failed to connect to redis for tests")
}

/// An agent node whose `tools` entry references a saved workspace MCP connection by id
/// (rather than embedding a raw url/token) should have the worker resolve it — decrypting
/// the stored token — before calling the tool, against a real MCP server over HTTP.
#[sqlx::test(migrations = "../api/migrations")]
async fn resolves_saved_connection_and_calls_its_tool(pool: PgPool) {
    let ct = CancellationToken::new();
    let service: StreamableHttpService<WeatherServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(WeatherServer::new()),
            Default::default(),
            StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    let workspace_id: Uuid =
        sqlx::query_scalar("INSERT INTO workspaces (name) VALUES ('Acme') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();

    let encrypted_token = mcp::encrypt_token(MCP_CREDENTIAL_KEY, "shh-its-a-secret").unwrap();
    let connection_id: Uuid = sqlx::query_scalar(
        "INSERT INTO mcp_connections (workspace_id, name, url, encrypted_bearer_token)
         VALUES ($1, 'weather', $2, $3) RETURNING id",
    )
    .bind(workspace_id)
    .bind(format!("http://{addr}/mcp"))
    .bind(&encrypted_token)
    .fetch_one(&pool)
    .await
    .unwrap();

    let workflow_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflows (workspace_id, name) VALUES ($1, 'wf') RETURNING id",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let node_config = serde_json::json!({
        "prompt": "weather report: {{weather}}",
        "model": "test-model",
        "tools": [{ "mcp_connection_id": connection_id, "tool": "weather", "arguments": { "city": "Boston" } }],
    });
    let node_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_nodes (workflow_id, node_type, config) VALUES ($1, 'agent', $2) RETURNING id",
    )
    .bind(workflow_id)
    .bind(&node_config)
    .fetch_one(&pool)
    .await
    .unwrap();

    let execution_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_executions (workflow_id, status) VALUES ($1, 'pending') RETURNING id",
    )
    .bind(workflow_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    struct EchoProvider;
    #[async_trait::async_trait]
    impl llm::LlmProvider for EchoProvider {
        async fn complete(
            &self,
            request: &llm::CompletionRequest,
        ) -> Result<String, llm::LlmError> {
            Ok(request.messages[0].content.clone())
        }
        fn name(&self) -> &'static str {
            "echo"
        }
    }
    let provider: std::sync::Arc<dyn llm::LlmProvider> = std::sync::Arc::new(EchoProvider);

    let mut redis = redis_conn().await;
    let job = queue::RunJob {
        execution_id,
        workflow_id,
    };
    worker::run_job(&pool, &mut redis, &job, Some(&provider), MCP_CREDENTIAL_KEY)
        .await
        .unwrap();

    ct.cancel();
    server_handle.abort();

    let (status, output): (String, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT status, output FROM workflow_execution_nodes WHERE execution_id = $1 AND node_id = $2",
    )
    .bind(execution_id)
    .bind(node_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(status, "succeeded");
    assert_eq!(
        output,
        Some(serde_json::json!({ "response": "weather report: sunny in Boston" }))
    );
}

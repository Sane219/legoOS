use mcp::McpClient;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EchoRequest {
    message: String,
}

#[derive(Debug, Clone)]
struct EchoServer {
    tool_router: ToolRouter<Self>,
}

impl EchoServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl EchoServer {
    #[tool(description = "Echoes the given message back")]
    fn echo(&self, Parameters(EchoRequest { message }): Parameters<EchoRequest>) -> String {
        message
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
}

/// Runs a real MCP server (rmcp's own streamable-HTTP server transport) in-process and
/// drives our `McpClient` against it over a real TCP connection, proving list_tools and
/// call_tool actually round-trip the wire protocol rather than just type-checking.
#[tokio::test]
async fn lists_tools_and_calls_one_on_a_real_mcp_server() -> anyhow::Result<()> {
    let ct = CancellationToken::new();
    let service: StreamableHttpService<EchoServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(EchoServer::new()),
            Default::default(),
            StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let server_handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    let client = McpClient::connect(&format!("http://{addr}/mcp"), None).await?;

    let tools = client.list_tools().await?;
    assert!(tools.iter().any(|t| t.name == "echo"));

    let result = client
        .call_tool("echo", serde_json::json!({ "message": "hi" }))
        .await?;
    assert_eq!(result, serde_json::json!("hi"));

    client.close().await?;
    ct.cancel();
    server_handle.abort();
    Ok(())
}

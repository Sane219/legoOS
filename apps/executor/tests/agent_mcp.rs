use executor::{Edge, Node, execute};
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
use uuid::Uuid;

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

struct EchoProvider;

#[async_trait::async_trait]
impl llm::LlmProvider for EchoProvider {
    async fn complete(&self, request: &llm::CompletionRequest) -> Result<String, llm::LlmError> {
        Ok(request.messages[0].content.clone())
    }

    fn name(&self) -> &'static str {
        "echo"
    }
}

/// An agent node with a `tools` entry pointing at a real MCP server (rmcp's own
/// streamable-HTTP server transport, run in-process) should call it and splice the
/// result into the prompt template before the LLM ever sees it.
#[tokio::test]
async fn agent_node_calls_mcp_tool_and_renders_result_into_prompt() {
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

    let agent = Uuid::new_v4();
    let nodes = vec![Node {
        id: agent,
        node_type: "agent".to_string(),
        config: serde_json::json!({
            "prompt": "weather report: {{weather}}",
            "model": "test-model",
            "tools": [{
                "mcp_url": format!("http://{addr}/mcp"),
                "tool": "weather",
                "arguments": { "city": "Boston" },
            }],
        }),
    }];
    let edges: Vec<Edge> = vec![];

    let result = execute(&nodes, &edges, Some(&EchoProvider), None).await;

    ct.cancel();
    server_handle.abort();

    assert_eq!(result.status, executor::ExecutionStatus::Succeeded);
    let node_result = result.nodes.iter().find(|n| n.node_id == agent).unwrap();
    assert_eq!(
        node_result.output,
        Some(serde_json::json!({ "response": "weather report: sunny in Boston" }))
    );
}

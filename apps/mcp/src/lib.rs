use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientInfo, ContentBlock, Implementation},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("failed to connect to MCP server: {0}")]
    Connect(String),
    #[error("MCP request failed: {0}")]
    Request(String),
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// A connection to one MCP server over the Streamable HTTP transport, kept open for the
/// lifetime of this value so a caller can list tools once and call several without
/// re-negotiating a session each time.
pub struct McpClient {
    service: RunningService<RoleClient, ClientInfo>,
}

impl McpClient {
    pub async fn connect(url: &str, bearer_token: Option<&str>) -> Result<Self, McpError> {
        let mut config = StreamableHttpClientTransportConfig::with_uri(url);
        if let Some(token) = bearer_token {
            config = config.auth_header(token);
        }
        let transport = StreamableHttpClientTransport::from_config(config);

        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("legoos", env!("CARGO_PKG_VERSION")),
        );

        let service = client_info
            .serve(transport)
            .await
            .map_err(|e| McpError::Connect(e.to_string()))?;

        Ok(Self { service })
    }

    pub async fn list_tools(&self) -> Result<Vec<ToolInfo>, McpError> {
        let result = self
            .service
            .list_tools(Default::default())
            .await
            .map_err(|e| McpError::Request(e.to_string()))?;

        Ok(result
            .tools
            .into_iter()
            .map(|tool| ToolInfo {
                name: tool.name.to_string(),
                description: tool.description.map(|d| d.to_string()),
                input_schema: serde_json::to_value(tool.input_schema.as_ref())
                    .unwrap_or(Value::Null),
            })
            .collect())
    }

    /// Calls `tool_name` with `arguments` (must be a JSON object) and flattens the result's
    /// text content blocks back into a single JSON value for an agent node's context: one
    /// text block parses as JSON if possible (falling back to a plain string), multiple
    /// blocks become an array.
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpError> {
        let arguments = arguments.as_object().cloned().unwrap_or_default();

        let result = self
            .service
            .call_tool(CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments))
            .await
            .map_err(|e| McpError::Request(e.to_string()))?;

        let values: Vec<Value> = result
            .content
            .into_iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => {
                    Some(serde_json::from_str(&text.text).unwrap_or(Value::String(text.text)))
                }
                _ => None,
            })
            .collect();

        Ok(match values.len() {
            0 => Value::Null,
            1 => values.into_iter().next().expect("len checked above"),
            _ => Value::Array(values),
        })
    }

    pub async fn close(self) -> Result<(), McpError> {
        self.service
            .cancel()
            .await
            .map_err(|e| McpError::Connect(e.to_string()))?;
        Ok(())
    }
}

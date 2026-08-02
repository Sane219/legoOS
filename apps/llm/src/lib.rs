use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("missing configuration: {0}")]
    MissingConfig(String),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("provider returned an error response ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("provider returned an unexpected response shape: {0}")]
    UnexpectedResponse(String),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: &CompletionRequest) -> Result<String, LlmError>;

    fn name(&self) -> &'static str;
}

/// Cloud provider backed by the Anthropic Messages API.
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.anthropic.com".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::MissingConfig("ANTHROPIC_API_KEY".to_string()))?;
        Ok(Self::new(api_key))
    }

    fn request_body(request: &CompletionRequest) -> serde_json::Value {
        serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "messages": request.messages,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: &CompletionRequest) -> Result<String, LlmError> {
        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&Self::request_body(request))
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            return Err(LlmError::Api {
                status: status.as_u16(),
                body: body.to_string(),
            });
        }

        body.get("content")
            .and_then(|c| c.get(0))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .map(str::to_string)
            .ok_or_else(|| LlmError::UnexpectedResponse(body.to_string()))
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

/// Local runtime provider backed by an Ollama-compatible `/api/chat` endpoint.
pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        Self::new(base_url)
    }

    fn request_body(request: &CompletionRequest) -> serde_json::Value {
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: system.clone(),
            });
        }
        messages.extend(request.messages.iter().cloned());

        serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": false,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(&self, request: &CompletionRequest) -> Result<String, LlmError> {
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&Self::request_body(request))
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            return Err(LlmError::Api {
                status: status.as_u16(),
                body: body.to_string(),
            });
        }

        body.get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(str::to_string)
            .ok_or_else(|| LlmError::UnexpectedResponse(body.to_string()))
    }

    fn name(&self) -> &'static str {
        "ollama"
    }
}

/// Selects a provider based on the `LLM_PROVIDER` env var (`anthropic` | `ollama`,
/// defaults to `anthropic`). Node configs then just pick a model name for that provider.
pub fn provider_from_env() -> Result<Box<dyn LlmProvider>, LlmError> {
    let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
    match provider.as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::from_env()?)),
        "ollama" => Ok(Box::new(OllamaProvider::from_env())),
        other => Err(LlmError::UnknownProvider(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CompletionRequest {
        CompletionRequest {
            model: "test-model".to_string(),
            system: Some("be terse".to_string()),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            max_tokens: 128,
        }
    }

    #[test]
    fn anthropic_request_body_has_top_level_system() {
        let body = AnthropicProvider::request_body(&sample_request());
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn ollama_request_body_folds_system_into_messages() {
        let body = OllamaProvider::request_body(&sample_request());
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "be terse");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn provider_from_env_rejects_unknown_provider() {
        // SAFETY: tests run single-threaded within this module's process; no other test
        // in this crate reads LLM_PROVIDER concurrently.
        unsafe {
            std::env::set_var("LLM_PROVIDER", "bogus");
        }
        let result = provider_from_env();
        unsafe {
            std::env::remove_var("LLM_PROVIDER");
        }
        assert!(matches!(result, Err(LlmError::UnknownProvider(_))));
    }
}

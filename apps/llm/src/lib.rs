use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embeds one piece of text into a dense vector. Every vector returned by a given
    /// provider/model has the same length — `rag::RagClient::ensure_collection` is told
    /// that length once, up front.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError>;

    fn name(&self) -> &'static str;
}

/// Local runtime provider backed by an Ollama-compatible `/api/embeddings` endpoint.
pub struct OllamaEmbeddingProvider {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaEmbeddingProvider {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model =
            std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        Self::new(base_url, model)
    }
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError> {
        let response = self
            .client
            .post(format!("{}/api/embeddings", self.base_url))
            .json(&serde_json::json!({ "model": self.model, "prompt": text }))
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

        parse_embedding_array(body.get("embedding"))
            .ok_or_else(|| LlmError::UnexpectedResponse(body.to_string()))
    }

    fn name(&self) -> &'static str {
        "ollama"
    }
}

fn parse_embedding_array(value: Option<&Value>) -> Option<Vec<f32>> {
    value.and_then(Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(Value::as_f64)
            .map(|f| f as f32)
            .collect()
    })
}

/// Cloud provider backed by Voyage AI's embeddings API (Anthropic's recommended embeddings
/// partner — Anthropic itself doesn't offer an embeddings endpoint).
pub struct VoyageEmbeddingProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl VoyageEmbeddingProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("VOYAGE_API_KEY")
            .map_err(|_| LlmError::MissingConfig("VOYAGE_API_KEY".to_string()))?;
        let model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "voyage-3".to_string());
        Ok(Self::new(api_key, model))
    }
}

#[async_trait]
impl EmbeddingProvider for VoyageEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError> {
        let response = self
            .client
            .post("https://api.voyageai.com/v1/embeddings")
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({ "input": [text], "model": self.model }))
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

        parse_embedding_array(
            body.get("data")
                .and_then(|d| d.get(0))
                .and_then(|d| d.get("embedding")),
        )
        .ok_or_else(|| LlmError::UnexpectedResponse(body.to_string()))
    }

    fn name(&self) -> &'static str {
        "voyage"
    }
}

/// Selects a provider based on the `EMBEDDING_PROVIDER` env var (`ollama` | `voyage`,
/// defaults to `ollama` since it's local and free). `EMBEDDING_MODEL` overrides the
/// per-provider default model name.
pub fn embedding_provider_from_env() -> Result<Box<dyn EmbeddingProvider>, LlmError> {
    let provider = std::env::var("EMBEDDING_PROVIDER").unwrap_or_else(|_| "ollama".to_string());
    match provider.as_str() {
        "ollama" => Ok(Box::new(OllamaEmbeddingProvider::from_env())),
        "voyage" => Ok(Box::new(VoyageEmbeddingProvider::from_env()?)),
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

    #[test]
    fn parse_embedding_array_reads_a_json_number_array() {
        let value = serde_json::json!([0.1, 0.2, -0.3]);
        assert_eq!(
            parse_embedding_array(Some(&value)),
            Some(vec![0.1, 0.2, -0.3])
        );
    }

    #[test]
    fn parse_embedding_array_rejects_missing_or_wrong_shape() {
        assert_eq!(parse_embedding_array(None), None);
        assert_eq!(
            parse_embedding_array(Some(&serde_json::json!("not an array"))),
            None
        );
    }

    #[test]
    fn ollama_embedding_response_shape_parses() {
        let body = serde_json::json!({ "embedding": [1.0, 2.0, 3.0] });
        assert_eq!(
            parse_embedding_array(body.get("embedding")),
            Some(vec![1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn voyage_embedding_response_shape_parses() {
        let body = serde_json::json!({ "data": [{ "embedding": [1.0, 2.0, 3.0] }] });
        let extracted = body
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"));
        assert_eq!(parse_embedding_array(extracted), Some(vec![1.0, 2.0, 3.0]));
    }

    #[test]
    fn embedding_provider_from_env_rejects_unknown_provider() {
        // SAFETY: see provider_from_env_rejects_unknown_provider above.
        unsafe {
            std::env::set_var("EMBEDDING_PROVIDER", "bogus");
        }
        let result = embedding_provider_from_env();
        unsafe {
            std::env::remove_var("EMBEDDING_PROVIDER");
        }
        assert!(matches!(result, Err(LlmError::UnknownProvider(_))));
    }
}

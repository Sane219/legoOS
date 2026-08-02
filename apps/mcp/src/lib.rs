use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use rand::Rng;
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

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("MCP_CREDENTIAL_KEY must be 64 hex characters (32 bytes): {0}")]
    InvalidKey(String),
    #[error("stored credential is corrupt or was encrypted with a different key")]
    Decrypt,
}

/// Encrypts an MCP server's bearer token before it's stored (AES-256-GCM, a random nonce
/// per call). `key_hex` is `MCP_CREDENTIAL_KEY`: 64 hex characters. Output is
/// `base64(nonce || ciphertext)`, safe to put straight in a TEXT column.
pub fn encrypt_token(key_hex: &str, plaintext: &str) -> Result<String, CredentialError> {
    let cipher = cipher_from_hex_key(key_hex)?;
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| CredentialError::Decrypt)?;

    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        payload,
    ))
}

/// Reverses [`encrypt_token`].
pub fn decrypt_token(key_hex: &str, encoded: &str) -> Result<String, CredentialError> {
    let cipher = cipher_from_hex_key(key_hex)?;
    let payload = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
        .map_err(|_| CredentialError::Decrypt)?;

    if payload.len() < 12 {
        return Err(CredentialError::Decrypt);
    }
    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let nonce = Nonce::try_from(nonce_bytes).map_err(|_| CredentialError::Decrypt)?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| CredentialError::Decrypt)?;
    String::from_utf8(plaintext).map_err(|_| CredentialError::Decrypt)
}

fn cipher_from_hex_key(key_hex: &str) -> Result<Aes256Gcm, CredentialError> {
    let bytes = hex_decode(key_hex)
        .ok_or_else(|| CredentialError::InvalidKey(key_hex.len().to_string()))?;
    if bytes.len() != 32 {
        return Err(CredentialError::InvalidKey(format!(
            "{} bytes",
            bytes.len()
        )));
    }
    let key = Key::<Aes256Gcm>::try_from(bytes.as_slice())
        .map_err(|_| CredentialError::InvalidKey("32 bytes".to_string()))?;
    Ok(Aes256Gcm::new(&key))
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_key() -> &'static str {
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            .get(0..64)
            .unwrap()
    }

    fn other_key() -> &'static str {
        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            .get(0..64)
            .unwrap()
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let encrypted = encrypt_token(valid_key(), "super-secret-token").unwrap();
        assert_ne!(encrypted, "super-secret-token");
        assert_eq!(
            decrypt_token(valid_key(), &encrypted).unwrap(),
            "super-secret-token"
        );
    }

    #[test]
    fn two_encryptions_of_the_same_token_differ() {
        let a = encrypt_token(valid_key(), "same-token").unwrap();
        let b = encrypt_token(valid_key(), "same-token").unwrap();
        assert_ne!(a, b, "nonce should be random per call");
    }

    #[test]
    fn decrypt_fails_with_the_wrong_key() {
        let encrypted = encrypt_token(valid_key(), "super-secret-token").unwrap();
        assert!(decrypt_token(other_key(), &encrypted).is_err());
    }

    #[test]
    fn rejects_a_key_of_the_wrong_length() {
        assert!(matches!(
            encrypt_token("deadbeef", "x"),
            Err(CredentialError::InvalidKey(_))
        ));
    }
}

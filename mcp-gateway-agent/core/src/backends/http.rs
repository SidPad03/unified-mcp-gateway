//! A local MCP server spoken to over HTTP.
//!
//! There is no process to supervise here, so "running" means the last
//! `initialize` succeeded. A backend that goes away silently is noticed on the
//! next tool call, and the Backends page offers Restart to re-probe it.

use std::time::Duration;

use serde_json::Value;

use crate::config::LocalBackendConfig;

pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
pub const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(30);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct HttpClient {
    url: String,
    // `reqwest::Client` is an Arc internally, so cloning this is cheap and the
    // connection pool is shared.
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new(config: &LocalBackendConfig) -> Result<Self, String> {
        let url = config
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| format!("Backend '{}' has no URL", config.name))?
            .to_string();

        let mut builder = reqwest::Client::builder();
        if !config.headers.is_empty() {
            let mut headers = reqwest::header::HeaderMap::new();
            for (key, value) in &config.headers {
                let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| format!("'{key}' is not a valid header name"))?;
                let value = reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| format!("Header '{key}' has a value HTTP cannot carry"))?;
                headers.insert(name, value);
            }
            builder = builder.default_headers(headers);
        }

        let client = builder
            .build()
            .map_err(|e| format!("Could not build the HTTP client: {e}"))?;

        Ok(Self { url, client })
    }

    async fn rpc(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": method,
            "params": params,
        });

        let response = self
            .client
            .post(&self.url)
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| format!("{method} request failed: {e}"))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("Could not read the {method} response: {e}"))?;

        if !status.is_success() {
            let detail = text.chars().take(300).collect::<String>();
            return Err(format!("HTTP {status}: {detail}"));
        }

        let parsed: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{method} did not return JSON: {e}"))?;

        if let Some(error) = parsed.get("error") {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            return Err(format!("JSON-RPC error: {message}"));
        }

        Ok(parsed.get("result").cloned().unwrap_or(parsed))
    }

    pub async fn initialize(&self) -> Result<(), String> {
        self.rpc("initialize", super::initialize_params(), INITIALIZE_TIMEOUT)
            .await?;
        // Best effort: a server that rejects the notification still works.
        let _ = self
            .client
            .post(&self.url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Value, String> {
        self.rpc("tools/list", serde_json::json!({}), LIST_TOOLS_TIMEOUT)
            .await
    }

    pub async fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        self.rpc(
            "tools/call",
            serde_json::json!({ "name": tool, "arguments": arguments }),
            CALL_TIMEOUT,
        )
        .await
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_backend_without_a_url() {
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            ..Default::default()
        };
        assert!(HttpClient::new(&config).is_err());
    }

    #[test]
    fn rejects_a_header_name_http_cannot_carry() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("bad header".to_string(), "value".to_string());
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            url: Some("http://127.0.0.1:1/mcp".into()),
            headers,
            ..Default::default()
        };
        let err = HttpClient::new(&config).err().expect("should be rejected");
        assert!(err.contains("bad header"), "{err}");
    }

    #[tokio::test]
    async fn a_refused_connection_is_an_error_not_a_success() {
        // Port 1 on loopback: nothing listens, and connect() fails fast.
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            url: Some("http://127.0.0.1:1/mcp".into()),
            ..Default::default()
        };
        let client = HttpClient::new(&config).unwrap();
        assert!(client.initialize().await.is_err());
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::config::LocalBackendConfig;

// ── Tool info (sent during registration) ────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Sub-backend metadata sent to the gateway during registration.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubBackendInfo {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Only env key names (values are sensitive)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    pub tool_count: usize,
}

// ── Stdio process wrapper ───────────────────────────────────────────────

struct StdioProcess {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

struct StdioBackend {
    process: Arc<Mutex<StdioProcess>>,
}

// ── HTTP backend ────────────────────────────────────────────────────────

struct HttpBackend {
    url: String,
    client: reqwest::Client,
}

// ── Local backend manager ───────────────────────────────────────────────

enum BackendHandle {
    Stdio(StdioBackend),
    Http(HttpBackend),
}

pub struct LocalBackendManager {
    backends: HashMap<String, BackendHandle>,
    /// Maps "backend_name__tool_name" → backend_name
    tool_routes: HashMap<String, String>,
    /// All discovered tools (namespaced as "backend__tool")
    all_tools: Vec<ToolInfo>,
    /// Original configs for generating sub-backend info
    configs: Vec<LocalBackendConfig>,
    /// Tool counts per sub-backend
    tool_counts: HashMap<String, usize>,
}

impl LocalBackendManager {
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
            tool_routes: HashMap::new(),
            all_tools: Vec::new(),
            configs: Vec::new(),
            tool_counts: HashMap::new(),
        }
    }

    /// Start all configured local backends and discover their tools.
    pub async fn start_all(&mut self, configs: &[LocalBackendConfig]) -> anyhow::Result<()> {
        self.configs = configs.to_vec();
        for config in configs {
            match config.transport.as_str() {
                "stdio" => self.start_stdio(config).await?,
                "http" | "streamable-http" => self.start_http(config).await?,
                other => {
                    tracing::warn!(backend = %config.name, transport = %other, "Unknown transport, skipping");
                }
            }
        }
        Ok(())
    }

    /// All tools discovered from local backends (namespaced).
    pub fn all_tools(&self) -> &[ToolInfo] {
        &self.all_tools
    }

    /// Sub-backend metadata for the register message.
    pub fn sub_backends(&self) -> Vec<SubBackendInfo> {
        self.configs.iter().map(|c| {
            SubBackendInfo {
                name: c.name.clone(),
                transport: c.transport.clone(),
                command: c.command.clone(),
                args: c.args.clone(),
                url: c.url.clone(),
                env_keys: c.env.keys().cloned().collect(),
                tool_count: self.tool_counts.get(&c.name).copied().unwrap_or(0),
            }
        }).collect()
    }

    /// Route a tool call to the correct local backend.
    /// `tool_name` is the namespaced name "backend__original_tool".
    pub async fn call_tool(&self, tool_name: &str, arguments: &Value) -> Result<Value, String> {
        let backend_name = self
            .tool_routes
            .get(tool_name)
            .ok_or_else(|| format!("No route for tool '{}'", tool_name))?;

        // Extract the original (un-namespaced) tool name
        let prefix = format!("{}__{}", backend_name, "");
        let original_name = tool_name.strip_prefix(&prefix).unwrap_or(tool_name);

        let handle = self
            .backends
            .get(backend_name)
            .ok_or_else(|| format!("Backend '{}' not found", backend_name))?;

        match handle {
            BackendHandle::Stdio(stdio) => {
                Self::call_stdio_tool(&stdio.process, original_name, arguments).await
            }
            BackendHandle::Http(http) => {
                Self::call_http_tool(&http.client, &http.url, original_name, arguments).await
            }
        }
    }

    pub async fn shutdown_all(&mut self) {
        for (name, handle) in self.backends.drain() {
            match handle {
                BackendHandle::Stdio(stdio) => {
                    let mut proc = stdio.process.lock().await;
                    tracing::info!(backend = %name, "Stopping local stdio backend");
                    let _ = proc._child.kill().await;
                }
                BackendHandle::Http(_) => {
                    tracing::info!(backend = %name, "Closing local HTTP backend");
                }
            }
        }
    }

    // ── Stdio backend ───────────────────────────────────────────────────

    async fn start_stdio(&mut self, config: &LocalBackendConfig) -> anyhow::Result<()> {
        let command = config
            .command
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Stdio backend '{}' missing 'command'", config.name))?;

        tracing::info!(
            backend = %config.name,
            command = %command,
            args = ?config.args,
            "Spawning local stdio backend"
        );

        let mut cmd = Command::new(command);
        cmd.args(&config.args)
            .envs(&config.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn '{}': {}", command, e))?;

        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdin"))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;

        let process = Arc::new(Mutex::new(StdioProcess {
            _child: child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
        }));

        // Initialize MCP
        let init_result = Self::jsonrpc_call(
            &process,
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway-agent", "version": "0.1.0" }
            })),
        )
        .await;

        match &init_result {
            Ok(resp) => tracing::info!(backend = %config.name, ?resp, "MCP initialize succeeded"),
            Err(e) => {
                tracing::error!(backend = %config.name, error = %e, "MCP initialize failed");
                return Err(anyhow::anyhow!("Initialize failed: {}", e));
            }
        }

        // Send initialized notification
        let _ = Self::jsonrpc_notify(&process, "notifications/initialized", None).await;

        // Discover tools
        let tools = self.discover_stdio_tools(&process, &config.name).await?;
        self.tool_counts.insert(config.name.clone(), tools.len());

        self.backends.insert(
            config.name.clone(),
            BackendHandle::Stdio(StdioBackend { process }),
        );

        tracing::info!(
            backend = %config.name,
            tool_count = tools.len(),
            "Local stdio backend started"
        );
        Ok(())
    }

    async fn discover_stdio_tools(
        &mut self,
        process: &Arc<Mutex<StdioProcess>>,
        backend_name: &str,
    ) -> anyhow::Result<Vec<ToolInfo>> {
        let tools_result =
            Self::jsonrpc_call(process, "tools/list", Some(serde_json::json!({}))).await;

        let raw_tools = match tools_result {
            Ok(resp) => resp
                .get("tools")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!(backend = %backend_name, error = %e, "tools/list failed");
                vec![]
            }
        };

        let mut tools = Vec::new();
        for t in &raw_tools {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let namespaced = format!("{}__{}", backend_name, name);
            let description = t
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

            let tool = ToolInfo {
                name: namespaced.clone(),
                description,
                input_schema,
            };
            self.tool_routes
                .insert(namespaced, backend_name.to_string());
            tools.push(tool.clone());
            self.all_tools.push(tool);
        }

        Ok(tools)
    }

    async fn call_stdio_tool(
        process: &Arc<Mutex<StdioProcess>>,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Value, String> {
        Self::jsonrpc_call(
            process,
            "tools/call",
            Some(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            })),
        )
        .await
    }

    async fn jsonrpc_call(
        process: &Arc<Mutex<StdioProcess>>,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, String> {
        let mut proc = process.lock().await;
        let id = uuid::Uuid::new_v4().to_string();

        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            request["params"] = p;
        }

        let mut line = serde_json::to_string(&request).map_err(|e| format!("Serialize: {}", e))?;
        line.push('\n');

        proc.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write to stdin: {}", e))?;
        proc.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush stdin: {}", e))?;

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
        loop {
            let mut response_line = String::new();
            let read_future = proc.stdout.read_line(&mut response_line);

            match tokio::time::timeout_at(deadline, read_future).await {
                Ok(Ok(0)) => return Err("Backend closed stdout (EOF)".into()),
                Ok(Ok(_)) => {
                    let trimmed = response_line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let parsed: Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    // Skip notifications (no id)
                    if parsed.get("id").is_none()
                        || parsed.get("id") == Some(&Value::Null)
                    {
                        continue;
                    }
                    if let Some(error) = parsed.get("error") {
                        let msg = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error");
                        return Err(format!("JSON-RPC error: {}", msg));
                    }
                    return Ok(parsed
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::json!({})));
                }
                Ok(Err(e)) => return Err(format!("Read from stdout: {}", e)),
                Err(_) => return Err("Timeout (60s) waiting for backend response".into()),
            }
        }
    }

    async fn jsonrpc_notify(
        process: &Arc<Mutex<StdioProcess>>,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let mut proc = process.lock().await;
        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(p) = params {
            request["params"] = p;
        }
        let mut line = serde_json::to_string(&request).map_err(|e| format!("Serialize: {}", e))?;
        line.push('\n');
        proc.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write: {}", e))?;
        proc.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush: {}", e))?;
        Ok(())
    }

    // ── HTTP backend ────────────────────────────────────────────────────

    async fn start_http(&mut self, config: &LocalBackendConfig) -> anyhow::Result<()> {
        let url = config
            .url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("HTTP backend '{}' missing 'url'", config.name))?;

        tracing::info!(backend = %config.name, url = %url, "Connecting to local HTTP backend");

        let mut builder = reqwest::Client::builder();
        if !config.headers.is_empty() {
            let mut header_map = reqwest::header::HeaderMap::new();
            for (key, val) in &config.headers {
                if let (Ok(name), Ok(value)) = (
                    reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                    reqwest::header::HeaderValue::from_str(val),
                ) {
                    header_map.insert(name, value);
                }
            }
            builder = builder.default_headers(header_map);
        }
        let client = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        // Initialize
        let init_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway-agent", "version": "0.1.0" }
            }
        });

        let init_resp = client
            .post(url)
            .json(&init_body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP initialize failed: {}", e))?;

        if !init_resp.status().is_success() {
            let status = init_resp.status();
            let body = init_resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Initialize returned HTTP {}: {}",
                status,
                body
            ));
        }

        tracing::info!(backend = %config.name, "HTTP MCP initialize succeeded");

        // Send initialized notification
        let notif_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let _ = client
            .post(url)
            .json(&notif_body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        // Discover tools
        let tools_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let tools_resp = client
            .post(url)
            .json(&tools_body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP tools/list failed: {}", e))?;

        let tools_json: Value = tools_resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse tools/list: {}", e))?;

        let result = tools_json.get("result").unwrap_or(&tools_json);
        let tool_array = result.get("tools").and_then(|t| t.as_array());

        let mut tool_count = 0usize;
        if let Some(arr) = tool_array {
            for t in arr {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let namespaced = format!("{}__{}", config.name, name);
                let description = t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

                let tool = ToolInfo {
                    name: namespaced.clone(),
                    description,
                    input_schema,
                };
                self.tool_routes
                    .insert(namespaced, config.name.clone());
                self.all_tools.push(tool);
                tool_count += 1;
            }
        }

        self.tool_counts.insert(config.name.clone(), tool_count);

        self.backends.insert(
            config.name.clone(),
            BackendHandle::Http(HttpBackend {
                url: url.to_string(),
                client,
            }),
        );

        tracing::info!(
            backend = %config.name,
            tool_count,
            "Local HTTP backend started"
        );
        Ok(())
    }

    async fn call_http_tool(
        client: &reqwest::Client,
        url: &str,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Value, String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        let resp = client
            .post(url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = resp.status();
        let resp_json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if !status.is_success() {
            return Err(format!("HTTP {}: {}", status, resp_json));
        }

        if let Some(result) = resp_json.get("result") {
            Ok(result.clone())
        } else if let Some(error) = resp_json.get("error") {
            Err(format!("Backend error: {}", error))
        } else {
            Ok(resp_json)
        }
    }
}

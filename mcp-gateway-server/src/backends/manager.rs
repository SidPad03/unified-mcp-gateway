use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

struct StdioProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

pub struct RunningBackend {
    process: Arc<Mutex<StdioProcess>>,
    pub name: String,
}

pub struct BackendManager {
    backends: RwLock<HashMap<Uuid, Arc<RunningBackend>>>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl BackendManager {
    pub fn new() -> Self {
        Self {
            backends: RwLock::new(HashMap::new()),
        }
    }

    pub async fn spawn_backend(
        &self,
        backend_id: Uuid,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredTool>, String> {
        // Stop existing process if any
        self.stop_backend(&backend_id).await;

        let command = config
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("Config missing 'command' field")?;
        let args: Vec<&str> = config
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let env_map: HashMap<String, String> = config
            .get("env")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        // Log arg count, not the args themselves: stdio MCP args commonly carry
        // tokens/secrets (env is already omitted for the same reason).
        tracing::info!(
            backend = name,
            command,
            arg_count = args.len(),
            "Spawning stdio backend"
        );

        let mut cmd = Command::new(command);
        cmd.args(&args)
            .envs(&env_map)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn '{}': {}", command, e))?;

        let child_stdin = child.stdin.take().ok_or("Failed to capture stdin")?;
        let child_stdout = child.stdout.take().ok_or("Failed to capture stdout")?;

        let proc = StdioProcess {
            child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
        };

        let process = Arc::new(Mutex::new(proc));
        let running = Arc::new(RunningBackend {
            process: process.clone(),
            name: name.to_string(),
        });

        self.backends.write().await.insert(backend_id, running);

        // Initialize the MCP server
        let init_result = Self::jsonrpc_call(
            &process,
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway", "version": "0.1.0" }
            })),
        )
        .await;

        match &init_result {
            Ok(resp) => tracing::info!(backend = name, ?resp, "MCP initialize succeeded"),
            Err(e) => {
                tracing::error!(backend = name, error = %e, "MCP initialize failed");
                self.stop_backend(&backend_id).await;
                return Err(format!("Initialize failed: {}", e));
            }
        }

        // Send initialized notification
        let _ = Self::jsonrpc_notify(&process, "notifications/initialized", None).await;

        // Discover tools
        let tools_result =
            Self::jsonrpc_call(&process, "tools/list", Some(serde_json::json!({}))).await;

        let tools = match tools_result {
            Ok(resp) => {
                let tool_array = resp.get("tools").and_then(|t| t.as_array());
                match tool_array {
                    Some(arr) => {
                        arr.iter()
                            .map(|t| DiscoveredTool {
                                name: t
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                description: t
                                    .get("description")
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                input_schema: t.get("inputSchema").cloned().unwrap_or(
                                    serde_json::json!({"type": "object", "properties": {}}),
                                ),
                            })
                            .filter(|t| !t.name.is_empty())
                            .collect()
                    }
                    None => {
                        tracing::warn!(backend = name, "tools/list returned no tools array");
                        vec![]
                    }
                }
            }
            Err(e) => {
                tracing::warn!(backend = name, error = %e, "tools/list failed, backend started but no tools discovered");
                vec![]
            }
        };

        tracing::info!(
            backend = name,
            tool_count = tools.len(),
            "Backend spawned and tools discovered"
        );
        Ok(tools)
    }

    pub async fn stop_backend(&self, backend_id: &Uuid) {
        if let Some(running) = self.backends.write().await.remove(backend_id) {
            let mut proc = running.process.lock().await;
            tracing::info!(backend = %running.name, "Stopping stdio backend");
            let _ = proc.child.kill().await;
        }
    }

    pub async fn call_tool(
        &self,
        backend_id: &Uuid,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Clone the process handle and release the read guard before the awaited
        // round-trip; holding it across `jsonrpc_call` (up to 60s) would block every
        // lifecycle writer (spawn/stop/shutdown) for the duration of the call.
        let process = {
            let backends = self.backends.read().await;
            backends
                .get(backend_id)
                .ok_or_else(|| "Backend process not running".to_string())?
                .process
                .clone()
        };

        let result = Self::jsonrpc_call(
            &process,
            "tools/call",
            Some(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            })),
        )
        .await?;

        Ok(result)
    }

    pub async fn is_running(&self, backend_id: &Uuid) -> bool {
        self.backends.read().await.contains_key(backend_id)
    }

    pub async fn shutdown_all(&self) {
        let mut backends = self.backends.write().await;
        for (_, running) in backends.drain() {
            let mut proc = running.process.lock().await;
            tracing::info!(backend = %running.name, "Shutting down stdio backend");
            let _ = proc.child.kill().await;
        }
    }

    /// Discover tools from an HTTP-based MCP backend (streamable-http or SSE).
    /// Sends initialize + tools/list via JSON-RPC POST to the configured URL.
    pub async fn discover_http_tools(
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredTool>, String> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("Config missing 'url' field")?;

        let client = Self::build_http_client(config)?;

        tracing::info!(backend = name, url, "Discovering tools from HTTP backend");

        // Step 1: Initialize
        let init_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway", "version": "0.1.0" }
            }
        });

        let init_resp = client
            .post(url)
            .json(&init_body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("HTTP initialize request failed: {}", e))?;

        if !init_resp.status().is_success() {
            let status = init_resp.status();
            let body = init_resp.text().await.unwrap_or_default();
            return Err(format!("Initialize returned HTTP {}: {}", status, body));
        }

        let init_json = Self::read_streamable_response(init_resp)
            .await
            .map_err(|e| format!("Failed to parse initialize response: {}", e))?;

        tracing::info!(backend = name, resp = ?init_json, "HTTP MCP initialize succeeded");

        // Step 2: Send initialized notification (fire and forget)
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

        // Step 3: Discover tools
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
            .map_err(|e| format!("HTTP tools/list request failed: {}", e))?;

        if !tools_resp.status().is_success() {
            let status = tools_resp.status();
            let body = tools_resp.text().await.unwrap_or_default();
            return Err(format!("tools/list returned HTTP {}: {}", status, body));
        }

        let tools_json = Self::read_streamable_response(tools_resp)
            .await
            .map_err(|e| format!("Failed to parse tools/list response: {}", e))?;

        // Parse tools from the result
        let result = tools_json.get("result").unwrap_or(&tools_json);
        let tool_array = result.get("tools").and_then(|t| t.as_array());

        let tools: Vec<DiscoveredTool> = match tool_array {
            Some(arr) => arr
                .iter()
                .map(|t| DiscoveredTool {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                    input_schema: t
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
                })
                .filter(|t| !t.name.is_empty())
                .collect(),
            None => {
                tracing::warn!(backend = name, "HTTP tools/list returned no tools array");
                vec![]
            }
        };

        tracing::info!(
            backend = name,
            tool_count = tools.len(),
            "HTTP backend tools discovered"
        );
        Ok(tools)
    }

    /// Forward a tool call to a streamable-http MCP backend.
    pub async fn call_http_tool(
        config: &serde_json::Value,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("Backend config missing 'url' field")?;

        let client = Self::build_http_client(config)?;

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
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Backend request failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Backend returned HTTP {}: {}", status, body));
        }

        let resp_json = Self::read_streamable_response(resp)
            .await
            .map_err(|e| format!("Failed to parse backend response: {}", e))?;

        if let Some(result) = resp_json.get("result") {
            Ok(result.clone())
        } else if let Some(error) = resp_json.get("error") {
            Err(format!("Backend error: {}", error))
        } else {
            Ok(resp_json)
        }
    }

    /// Discover tools from an SSE MCP backend using the proper SSE protocol:
    /// GET to establish stream -> read endpoint event -> POST JSON-RPC to that endpoint.
    pub async fn discover_sse_tools(
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredTool>, String> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("Config missing 'url' field")?;

        let client = Self::build_http_client(config)?;

        tracing::info!(backend = name, url, "Discovering tools from SSE backend");

        let mut sse = SseConnection::connect(&client, url).await?;

        // Initialize
        let init_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway", "version": "0.1.0" }
            }
        });

        client
            .post(&sse.post_url)
            .json(&init_body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("SSE POST initialize failed: {}", e))?;

        let init_resp = sse.read_jsonrpc_response().await?;
        tracing::info!(backend = name, resp = ?init_resp, "SSE MCP initialize succeeded");

        // Send initialized notification (fire and forget)
        let notif_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let _ = client
            .post(&sse.post_url)
            .json(&notif_body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        // Discover tools
        let tools_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        client
            .post(&sse.post_url)
            .json(&tools_body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("SSE POST tools/list failed: {}", e))?;

        let tools_json = sse.read_jsonrpc_response().await?;

        let result = tools_json.get("result").unwrap_or(&tools_json);
        let tool_array = result.get("tools").and_then(|t| t.as_array());

        let tools: Vec<DiscoveredTool> = match tool_array {
            Some(arr) => arr
                .iter()
                .map(|t| DiscoveredTool {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                    input_schema: t
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
                })
                .filter(|t| !t.name.is_empty())
                .collect(),
            None => {
                tracing::warn!(backend = name, "SSE tools/list returned no tools array");
                vec![]
            }
        };

        tracing::info!(
            backend = name,
            tool_count = tools.len(),
            "SSE backend tools discovered"
        );
        Ok(tools)
    }

    /// Forward a tool call to an SSE MCP backend using the proper SSE protocol.
    pub async fn call_sse_tool(
        config: &serde_json::Value,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("Backend config missing 'url' field")?;

        let client = Self::build_http_client(config)?;

        let mut sse = SseConnection::connect(&client, url).await?;

        // SSE requires initialize before tool calls
        let init_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "mcp-gateway", "version": "0.1.0" }
            }
        });
        client
            .post(&sse.post_url)
            .json(&init_body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("SSE POST initialize failed: {}", e))?;
        let _ = sse.read_jsonrpc_response().await?;

        let _ = client
            .post(&sse.post_url)
            .json(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        // Send the tool call
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        client
            .post(&sse.post_url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("SSE POST tools/call failed: {}", e))?;

        let resp_json = sse.read_jsonrpc_response().await?;

        if let Some(result) = resp_json.get("result") {
            Ok(result.clone())
        } else if let Some(error) = resp_json.get("error") {
            Err(format!("Backend error: {}", error))
        } else {
            Ok(resp_json)
        }
    }

    /// Read a JSON-RPC reply from a streamable-http POST response.
    ///
    /// A streamable-http server answers each POST with either a single JSON
    /// object (`application/json`) or a one-shot SSE stream (`text/event-stream`),
    /// deciding per response -- n8n commonly returns the latter. We read the body
    /// once and parse it according to `Content-Type` so a `tools/call` doesn't fail
    /// to deserialize after `initialize` happened to come back as plain JSON.
    async fn read_streamable_response(
        resp: reqwest::Response,
    ) -> Result<serde_json::Value, String> {
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        if content_type.contains("text/event-stream") {
            return Self::parse_sse_body(&body)
                .ok_or_else(|| format!("No JSON-RPC message in SSE response: {}", body));
        }

        // application/json (or unlabelled). Fall back to SSE framing if the body
        // isn't valid JSON but looks like an event stream.
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => Ok(v),
            Err(e) => Self::parse_sse_body(&body)
                .ok_or_else(|| format!("Failed to parse response body: {} (body: {})", e, body)),
        }
    }

    /// Extract the JSON-RPC payload from the `data:` frames of an SSE body,
    /// preferring a frame that carries a `result` or `error` over other messages
    /// (e.g. progress notifications) the server may interleave.
    fn parse_sse_body(body: &str) -> Option<serde_json::Value> {
        let mut fallback: Option<serde_json::Value> = None;
        for frame in body.split("\n\n") {
            let data = frame
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|r| r.trim())
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                if val.get("result").is_some() || val.get("error").is_some() {
                    return Some(val);
                }
                fallback.get_or_insert(val);
            }
        }
        fallback
    }

    fn build_http_client(config: &serde_json::Value) -> Result<reqwest::Client, String> {
        let mut builder = reqwest::Client::builder();

        // Attach custom request headers (e.g. Authorization) to every outbound call.
        // HTTP/SSE backends have no subprocess, so the KV pairs the dashboard form
        // stores under `env` are meaningless as environment variables -- they are only
        // useful as request headers. Fold both `env` and `headers` into the header map
        // so a stored `Authorization = Bearer <token>` pair actually reaches the backend.
        // `headers` is applied last, so an explicit header entry wins over an `env`
        // entry of the same name.
        let mut header_map = reqwest::header::HeaderMap::new();

        // The MCP streamable-http transport may answer any request with either a
        // plain JSON object or an SSE stream, chosen per response, so the client
        // must advertise that it accepts BOTH. Servers like n8n reject a POST with
        // 406 Not Acceptable unless `Accept` lists both media types. Seed it as a
        // default first (before user headers) so every streamable-http request --
        // initialize, tools/list, and tools/call alike -- carries it. Because we
        // insert user headers afterward, a stored `Accept` overwrites this default;
        // a missing one falls back to it. The SSE GET path sets its own per-request
        // `Accept: text/event-stream`, which reqwest keeps (a client default only
        // fills a header the request hasn't already set), so that path is unaffected.
        header_map.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json, text/event-stream"),
        );

        for field in ["env", "headers"] {
            if let Some(obj) = config.get(field).and_then(|h| h.as_object()) {
                for (key, val) in obj {
                    if let Some(val_str) = val.as_str() {
                        if let (Ok(name), Ok(mut value)) = (
                            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                            reqwest::header::HeaderValue::from_str(val_str),
                        ) {
                            value.set_sensitive(true); // keep bearer tokens out of debug logs
                            header_map.insert(name, value);
                        }
                    }
                }
            }
        }
        builder = builder.default_headers(header_map);

        builder
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))
    }

    async fn jsonrpc_call(
        process: &Arc<Mutex<StdioProcess>>,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
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

        let mut line =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {}", e))?;
        line.push('\n');

        proc.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write to stdin failed: {}", e))?;
        proc.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush stdin failed: {}", e))?;

        // Read response lines until we get a JSON-RPC response matching our ID
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
        loop {
            let mut response_line = String::new();
            let read_future = proc.stdout.read_line(&mut response_line);

            match tokio::time::timeout_at(deadline, read_future).await {
                Ok(Ok(0)) => return Err("Backend process closed stdout (EOF)".into()),
                Ok(Ok(_)) => {
                    let trimmed = response_line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(_) => continue, // skip non-JSON lines (e.g. log output)
                    };

                    // Accept only the response whose id matches the request we sent.
                    // This skips notifications (no id) AND server-initiated requests,
                    // which also carry an id (e.g. sampling/createMessage, roots/list).
                    // Critically, it also discards a STALE response left buffered in the
                    // shared pipe after a previous call timed out: without this check the
                    // next caller (possibly a different user) would receive that earlier
                    // caller's result, and the pipe would stay shifted by one for every
                    // later call until the backend was restarted. Mismatched lines are
                    // consumed and skipped, so the pipe self-resynchronizes.
                    match parsed.get("id") {
                        Some(serde_json::Value::String(resp_id)) if resp_id == &id => {}
                        _ => continue,
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
                Ok(Err(e)) => return Err(format!("Read from stdout failed: {}", e)),
                Err(_) => return Err("Timeout waiting for backend response (60s)".into()),
            }
        }
    }

    fn resolve_sse_url(base_url: &str, relative: &str) -> Result<String, String> {
        if relative.starts_with("http://") || relative.starts_with("https://") {
            // The `endpoint` event comes from the (untrusted) SSE backend, and the
            // gateway POSTs to it carrying the stored Authorization header (a
            // client-wide default header). Only accept an absolute URL if it stays
            // on the SAME ORIGIN as the configured backend, so a malicious or
            // compromised backend can't redirect our credentialed POST to an
            // attacker host (token exfiltration) or an internal SSRF target like
            // http://169.254.169.254/. Off-origin endpoints must use a relative
            // path, which always resolves back to the backend's own origin below.
            if url_origin(relative) == url_origin(base_url) && !url_origin(relative).is_empty() {
                return Ok(relative.to_string());
            }
            return Err(format!(
                "SSE backend returned a cross-origin endpoint URL ('{}'); refusing to send \
                 the backend's credentials off-origin",
                relative
            ));
        }
        // Extract scheme + host from base URL
        if let Some(idx) = base_url.find("://") {
            let after_scheme = &base_url[idx + 3..];
            if let Some(slash_idx) = after_scheme.find('/') {
                let origin = &base_url[..idx + 3 + slash_idx];
                if relative.starts_with('/') {
                    return Ok(format!("{}{}", origin, relative));
                }
                return Ok(format!("{}/{}", origin, relative));
            }
        }
        Ok(format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            relative.trim_start_matches('/')
        ))
    }

    async fn jsonrpc_notify(
        process: &Arc<Mutex<StdioProcess>>,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let mut proc = process.lock().await;

        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(p) = params {
            request["params"] = p;
        }

        let mut line =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {}", e))?;
        line.push('\n');

        proc.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write notification failed: {}", e))?;
        proc.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush notification failed: {}", e))?;

        Ok(())
    }
}

/// Manages an SSE connection to an MCP server, handling the GET stream
/// and extracting the POST endpoint URL from the initial `endpoint` event.
struct SseConnection {
    post_url: String,
    rx: tokio::sync::mpsc::Receiver<serde_json::Value>,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    async fn connect(client: &reqwest::Client, url: &str) -> Result<Self, String> {
        let response = client
            .get(url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| format!("SSE GET connect failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("SSE GET returned HTTP {}: {}", status, body));
        }

        let (tx, mut endpoint_rx) = tokio::sync::mpsc::channel::<SseEvent>(32);
        let base_url = url.to_string();

        let handle = tokio::spawn(async move {
            let mut buffer = String::new();
            let mut response = response;
            while let Ok(Some(chunk)) = response.chunk().await {
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find("\n\n") {
                    let event_text = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let mut event_type = String::new();
                    let mut data_lines = Vec::new();
                    for line in event_text.lines() {
                        if let Some(rest) = line.strip_prefix("event:") {
                            event_type = rest.trim().to_string();
                        } else if let Some(rest) = line.strip_prefix("data:") {
                            data_lines.push(rest.trim().to_string());
                        }
                    }
                    let data = data_lines.join("\n");

                    let event = SseEvent { event_type, data };
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
            }
        });

        // Wait for the `endpoint` event (up to 15 seconds)
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        let post_url = loop {
            match tokio::time::timeout_at(deadline, endpoint_rx.recv()).await {
                Ok(Some(event)) if event.event_type == "endpoint" => {
                    match BackendManager::resolve_sse_url(&base_url, &event.data) {
                        Ok(u) => break u,
                        Err(e) => return Err(e),
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => return Err("SSE stream closed before sending endpoint event".into()),
                Err(_) => return Err("Timeout (15s) waiting for SSE endpoint event".into()),
            }
        };

        tracing::info!(post_url = %post_url, "SSE endpoint discovered");

        // Convert the remaining events channel to only forward JSON-RPC messages
        let (json_tx, json_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(32);

        let forward_handle = tokio::spawn(async move {
            while let Some(event) = endpoint_rx.recv().await {
                if event.event_type == "message" || event.event_type.is_empty() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&event.data) {
                        if json_tx.send(json).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        // Detach the raw SSE reader task: dropping its JoinHandle does not cancel
        // the tokio task, so it keeps receiving; the forward task above drains it.
        drop(handle);

        Ok(Self {
            post_url,
            rx: json_rx,
            _handle: forward_handle,
        })
    }

    async fn read_jsonrpc_response(&mut self) -> Result<serde_json::Value, String> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
        loop {
            match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                Ok(Some(json)) => {
                    // Skip notifications (no id field)
                    if json.get("id").is_none() || json.get("id") == Some(&serde_json::Value::Null)
                    {
                        continue;
                    }
                    return Ok(json);
                }
                Ok(None) => return Err("SSE stream closed while waiting for response".into()),
                Err(_) => return Err("Timeout (30s) waiting for SSE JSON-RPC response".into()),
            }
        }
    }
}

struct SseEvent {
    event_type: String,
    data: String,
}

/// Return the origin (`scheme://host[:port]`) of a URL — everything before the
/// path — or "" when there is no scheme. Dependency-free parse in the same style
/// as `resolve_sse_url`; used to keep a credentialed SSE POST on the backend's
/// own origin.
fn url_origin(url: &str) -> &str {
    match url.find("://") {
        Some(idx) => {
            let rest = &url[idx + 3..];
            match rest.find('/') {
                Some(slash) => &url[..idx + 3 + slash],
                None => url,
            }
        }
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_same_origin_absolute_endpoint_is_allowed() {
        let out = BackendManager::resolve_sse_url(
            "https://tools.vendor.com/sse",
            "https://tools.vendor.com/messages?s=1",
        );
        assert_eq!(out.unwrap(), "https://tools.vendor.com/messages?s=1");
    }

    #[test]
    fn sse_relative_endpoint_resolves_to_backend_origin() {
        let out = BackendManager::resolve_sse_url("https://tools.vendor.com/sse", "/messages?s=1");
        assert_eq!(out.unwrap(), "https://tools.vendor.com/messages?s=1");
    }

    #[test]
    fn sse_cross_origin_absolute_endpoint_is_rejected() {
        // A malicious backend must not redirect the credentialed POST to another
        // host (bearer-token exfiltration / SSRF into internal targets).
        let out = BackendManager::resolve_sse_url(
            "https://tools.vendor.com/sse",
            "https://attacker.evil/collect",
        );
        assert!(
            out.is_err(),
            "cross-origin endpoint must be rejected: {out:?}"
        );
    }

    #[test]
    fn sse_scheme_downgrade_endpoint_is_rejected() {
        let out = BackendManager::resolve_sse_url(
            "https://tools.vendor.com/sse",
            "http://tools.vendor.com/messages",
        );
        assert!(
            out.is_err(),
            "https->http downgrade must be rejected: {out:?}"
        );
    }

    #[test]
    fn sse_body_extracts_jsonrpc_result_frame() {
        // n8n answers a streamable-http POST with a single SSE `message` event
        // carrying the JSON-RPC reply.
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n";
        let out = BackendManager::parse_sse_body(body).expect("should extract frame");
        assert_eq!(out["id"], 2);
        assert!(out.get("result").is_some());
    }

    #[test]
    fn sse_body_prefers_result_over_interleaved_notifications() {
        // A progress notification may precede the actual result frame; we must
        // return the frame with `result`, not the first parseable message.
        let body =
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{}}\n\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let out = BackendManager::parse_sse_body(body).expect("should extract result frame");
        assert_eq!(out["result"]["ok"], true);
    }

    #[test]
    fn sse_body_returns_error_frame() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"nope\"}}\n\n";
        let out = BackendManager::parse_sse_body(body).expect("should extract error frame");
        assert_eq!(out["error"]["code"], -32000);
    }

    #[test]
    fn sse_body_with_no_json_returns_none() {
        assert!(BackendManager::parse_sse_body(": keep-alive comment\n\n").is_none());
    }
}

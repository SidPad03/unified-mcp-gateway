use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::audit::redactor::Redactor;

struct StdioProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

pub struct RunningBackend {
    process: Arc<Mutex<StdioProcess>>,
    pub name: String,
}

/// Roughly a screen-hour of a chatty backend, per backend. The agent's
/// `LogBuffer` keeps 5 000; a gateway can be hosting a dozen of these at once,
/// so the per-process share is smaller.
const MAX_LOG_LINES: usize = 500;

/// One line a backend process wrote to stderr.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessLogLine {
    /// RFC 3339, UTC. The reader renders it in their own timezone.
    pub ts: String,
    pub text: String,
}

/// What is known about one stdio backend's process, kept whether or not it is
/// currently up.
///
/// It outlives the process deliberately: "why did it die" is a question you
/// only ever ask *after* it died, and a record that vanished with the child
/// left `gateway_get_mcp_server_logs` with nothing to say at exactly the moment
/// it was needed.
#[derive(Debug, Default, Clone)]
pub struct ProcessRecord {
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pid: Option<u32>,
    /// How many times this backend has been spawned in this process's lifetime,
    /// minus the first. A number that climbs on its own is the tell for a
    /// backend that starts and immediately dies.
    pub starts: u32,
    pub last_error: Option<String>,
    pub log: Vec<ProcessLogLine>,
    pub log_dropped: u64,
}

#[derive(Default)]
struct LogRing {
    lines: VecDeque<ProcessLogLine>,
    dropped: u64,
}

#[derive(Default)]
struct BackendTelemetry {
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    stopped_at: Option<chrono::DateTime<chrono::Utc>>,
    pid: Option<u32>,
    starts: u32,
    last_error: Option<String>,
    log: Arc<Mutex<LogRing>>,
}

pub struct BackendManager {
    backends: RwLock<HashMap<Uuid, Arc<RunningBackend>>>,
    /// Survives `stop_backend`, so the record of a process that has already
    /// gone is still readable.
    telemetry: RwLock<HashMap<Uuid, BackendTelemetry>>,
    /// Backends write credentials to stderr more often than anyone would like,
    /// and these lines are read back over MCP by an agent. Redact on the way
    /// *in*: a line is written once and may be read many times.
    redactor: Arc<Redactor>,
    /// The session each stateful streamable-http backend opened, so a call
    /// can join it rather than handshake again. A backend that has never
    /// opened one has no entry.
    http_sessions: RwLock<HashMap<Uuid, SessionSlot>>,
}

/// The header a streamable-http MCP server uses to hand out — and then demand
/// back — a session.
///
/// The SSE transport has no equivalent: there, the server announces a POST
/// endpoint over the stream (`event: endpoint`) and the session lives in that
/// URL, which `SseConnection` already posts every subsequent message to. So
/// this belongs to the streamable-http paths only.
const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

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
            telemetry: RwLock::new(HashMap::new()),
            redactor: Arc::new(Redactor::new()),
            http_sessions: RwLock::new(HashMap::new()),
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
        // The child inherits this process's environment, and a backend is
        // third-party code: strip the variables the gateway reads for itself
        // before adding the backend's own. Without this an `npx`-fetched server
        // could read JWT_SECRET and mint an owner token, or read DATABASE_URL
        // and reach Postgres past auth, policy and the audit trail.
        for key in crate::api::backends::GATEWAY_ONLY_ENV {
            cmd.env_remove(key);
        }
        cmd.args(&args)
            .envs(&env_map)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Captured rather than inherited so `gateway_get_mcp_server_logs`
            // has something to tail. A pipe nobody drains fills and wedges the
            // child, so the reader task below is not optional.
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // A spawn failure is recorded before it is returned: a backend that
        // never got a process is exactly the one whose status has to explain
        // why, and the caller only stores "unhealthy".
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let message = format!("Failed to spawn '{}': {}", command, e);
                self.note_error(backend_id, &message).await;
                return Err(message);
            }
        };

        let pid = child.id();
        let child_stdin = child.stdin.take().ok_or("Failed to capture stdin")?;
        let child_stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let child_stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

        let log = self.begin_run(backend_id, pid).await;
        Self::drain_stderr(child_stderr, log, Arc::clone(&self.redactor));

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

    /// Let a backend go: kill its process if it has one, and end its
    /// streamable-http session if it holds one.
    pub async fn stop_backend(&self, backend_id: &Uuid) {
        self.release_http_session(backend_id).await;
        if let Some(running) = self.backends.write().await.remove(backend_id) {
            let mut proc = running.process.lock().await;
            tracing::info!(backend = %running.name, "Stopping stdio backend");
            let _ = proc.child.kill().await;
        }
        if let Some(entry) = self.telemetry.write().await.get_mut(backend_id) {
            entry.stopped_at = Some(chrono::Utc::now());
            entry.pid = None;
        }
    }

    // ── Process telemetry ───────────────────────────────────────────────
    //
    // Everything `gateway_get_mcp_server_status` and
    // `gateway_get_mcp_server_logs` report comes from here. It is kept beside
    // the process map rather than inside it because the interesting questions —
    // why did it fail, what did it print before it went — are asked about
    // backends that are no longer in the process map at all.

    /// Note a new run: bump the start count, clear the previous failure, and
    /// hand back the ring its stderr should go into.
    async fn begin_run(&self, backend_id: Uuid, pid: Option<u32>) -> Arc<Mutex<LogRing>> {
        let mut telemetry = self.telemetry.write().await;
        let entry = telemetry.entry(backend_id).or_default();
        entry.started_at = Some(chrono::Utc::now());
        entry.stopped_at = None;
        entry.pid = pid;
        entry.starts += 1;
        entry.last_error = None;
        Arc::clone(&entry.log)
    }

    /// Record why a backend is not working, for the status tool to report.
    pub async fn note_error(&self, backend_id: Uuid, message: &str) {
        let mut telemetry = self.telemetry.write().await;
        let entry = telemetry.entry(backend_id).or_default();
        entry.last_error = Some(message.to_string());
    }

    /// What is known about one backend's process — `None` if the gateway has
    /// never tried to run it in this process's lifetime.
    pub async fn record(&self, backend_id: &Uuid, lines: usize) -> Option<ProcessRecord> {
        let telemetry = self.telemetry.read().await;
        let entry = telemetry.get(backend_id)?;
        let ring = entry.log.lock().await;
        let skip = ring.lines.len().saturating_sub(lines);
        Some(ProcessRecord {
            started_at: entry.started_at,
            stopped_at: entry.stopped_at,
            pid: entry.pid,
            starts: entry.starts,
            last_error: entry.last_error.clone(),
            log: ring.lines.iter().skip(skip).cloned().collect(),
            log_dropped: ring.dropped,
        })
    }

    /// Pump a child's stderr into its ring until the pipe closes.
    ///
    /// This task is what makes `Stdio::piped()` safe: an undrained pipe fills
    /// at 64 KiB and blocks the child mid-write, which for a chatty MCP server
    /// is a hang with no visible cause.
    fn drain_stderr(
        stderr: tokio::process::ChildStderr,
        ring: Arc<Mutex<LogRing>>,
        redactor: Arc<Redactor>,
    ) {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let line = ProcessLogLine {
                    ts: chrono::Utc::now().to_rfc3339(),
                    text: redactor.redact(&line),
                };
                let mut ring = ring.lock().await;
                ring.lines.push_back(line);
                while ring.lines.len() > MAX_LOG_LINES {
                    ring.lines.pop_front();
                    ring.dropped += 1;
                }
            }
        });
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

    /// Ask a *running* stdio backend for its tool list and report how many it
    /// has.
    ///
    /// This is what `gateway_test_backend_connectivity` uses instead of
    /// re-running discovery: discovery spawns, and spawning kills and replaces
    /// the process, which is a restart rather than a test. Asking the live
    /// process is both cheaper and honest about what is actually up.
    pub async fn probe(&self, backend_id: &Uuid) -> Result<usize, String> {
        let process = {
            let backends = self.backends.read().await;
            backends
                .get(backend_id)
                .ok_or_else(|| "Backend process is not running".to_string())?
                .process
                .clone()
        };

        let result =
            Self::jsonrpc_call(&process, "tools/list", Some(serde_json::json!({}))).await?;
        Ok(result
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.len())
            .unwrap_or(0))
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
        let now = chrono::Utc::now();
        for entry in self.telemetry.write().await.values_mut() {
            entry.stopped_at = Some(now);
            entry.pid = None;
        }
    }

    // ── Streamable HTTP ─────────────────────────────────────────────────

    /// Discover tools from a streamable-http MCP backend: `initialize`, then
    /// `tools/list` inside whatever session that opened.
    ///
    /// A session the server opens here is kept for this backend's tool calls
    /// rather than abandoned. The first call to a stateful server then does
    /// not pay for a second handshake, and the server is not left holding a
    /// session nobody will ever use until its own timeout reclaims it.
    pub async fn discover_http_tools(
        &self,
        backend_id: Uuid,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredTool>, String> {
        let (tools, session) = Self::list_http_tools(name, config).await?;
        match session {
            Some(id) => self.keep_http_session(backend_id, config, id).await,
            // Stateless now, whatever it was before an edit: a session kept
            // from an earlier configuration must not be sent to this one.
            None => self.release_http_session(&backend_id).await,
        }
        Ok(tools)
    }

    /// Discovery that leaves no trace: for `gateway_test_backend_connectivity`.
    ///
    /// It must not touch the session calls are using. Storing its own would
    /// `DELETE` that one out from under a call in flight — and on a backend
    /// that is switched off, leave a session in the cache that nothing ever
    /// releases. So it ends its own session and returns.
    pub async fn probe_http_tools(name: &str, config: &serde_json::Value) -> Result<usize, String> {
        let (tools, session) = Self::list_http_tools(name, config).await?;
        if let Some(id) = session {
            Self::end_http_session(HttpSession {
                config: config.clone(),
                id,
            });
        }
        Ok(tools.len())
    }

    /// The handshake and `tools/list`, returning the session they ran in. A
    /// session opened by a handshake whose `tools/list` then fails is ended
    /// here, since no caller will ever hear of it.
    async fn list_http_tools(
        name: &str,
        config: &serde_json::Value,
    ) -> Result<(Vec<DiscoveredTool>, Option<String>), String> {
        let url = Self::config_url(config)?;
        let client = Self::build_http_client(config)?;

        tracing::info!(backend = name, url, "Discovering tools from HTTP backend");

        // A stateful server assigns the session in the handshake and expects
        // it back on every request that belongs to it.
        let session = Self::http_handshake(&client, url).await?;
        if session.is_some() {
            tracing::debug!(backend = name, "Backend assigned an MCP session");
        }

        let listed = match Self::http_exchange(
            &client,
            url,
            session.as_deref(),
            &Self::rpc("tools/list", serde_json::json!({})),
            HTTP_TIMEOUT,
            "HTTP tools/list",
        )
        .await
        {
            Ok(HttpReply::Message(json)) => Ok(json),
            Ok(HttpReply::Refused(status, body)) => {
                Err(format!("tools/list returned HTTP {}: {}", status, body))
            }
            Err(e) => Err(e),
        };
        let tools_json = match listed {
            Ok(json) => json,
            Err(e) => {
                if let Some(id) = session {
                    Self::end_http_session(HttpSession {
                        config: config.clone(),
                        id,
                    });
                }
                return Err(e);
            }
        };

        let tools = Self::parse_tool_list(name, &tools_json, "streamable-http");
        tracing::info!(
            backend = name,
            tool_count = tools.len(),
            "HTTP backend tools discovered"
        );
        Ok((tools, session))
    }

    /// Forward a tool call to a streamable-http MCP backend.
    ///
    /// Most streamable-http servers are stateless and answer a bare
    /// `tools/call`. A **stateful** one refuses it, and there are two ways it
    /// says so. The spec's is a status: `400 Bad Request` when `Mcp-Session-Id`
    /// is missing, `404 Not Found` when the session it names has expired. The
    /// official Go SDK's is not. It answers `200 OK` with a JSON-RPC error —
    /// `method "tools/call" is invalid during session initialization` — which a
    /// status check never sees, so every call to such a server failed the same
    /// way however often it was retried (#13). Either refusal now buys one
    /// `initialize` and one retry inside the session it opens.
    ///
    /// The session is then kept for this backend and sent with every call
    /// after, so a stateful server costs one handshake rather than one per
    /// call. Opening a session per call was also leaving one behind on the
    /// server every time, to sit there until its idle timeout.
    pub async fn call_http_tool(
        &self,
        backend_id: Uuid,
        config: &serde_json::Value,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let url = Self::config_url(config)?;
        let client = Self::build_http_client(config)?;
        let body = Self::rpc(
            "tools/call",
            serde_json::json!({ "name": tool_name, "arguments": arguments }),
        );

        let slot = self.session_slot(backend_id).await;
        let sent = Self::session_for(&slot, config).await;

        let first = Self::http_exchange(
            &client,
            url,
            sent.as_deref(),
            &body,
            HTTP_TIMEOUT,
            "Backend",
        )
        .await?;
        if !Self::refused_for_want_of_session(&first, sent.is_some()) {
            return Self::tool_reply(first);
        }

        // Opened under the slot's lock, so a burst of calls that were all
        // refused together opens one session between them rather than one
        // each — the rest find it waiting when the lock comes free. That is
        // also what makes it safe to end the session being replaced: nobody
        // else can have opened it in the meantime.
        let session = {
            let mut guard = slot.lock().await;
            let current = guard
                .as_ref()
                .filter(|s| s.config == *config)
                .map(|s| s.id.clone());
            match current {
                Some(id) if Some(&id) != sent.as_ref() => id,
                _ => match Self::open_http_session(&client, url).await {
                    Ok(id) => {
                        let replaced = guard.replace(HttpSession {
                            config: config.clone(),
                            id: id.clone(),
                        });
                        // Refused, or opened under a configuration this
                        // backend no longer has; either way, finished.
                        if let Some(replaced) = replaced {
                            Self::end_http_session(replaced);
                        }
                        id
                    }
                    Err(e) => {
                        if let Some(replaced) = guard.take() {
                            Self::end_http_session(replaced);
                        }
                        return match first {
                            // A 200 whose error only read like a session
                            // refusal, from a server that turns out to have no
                            // sessions at all. That error was the backend's
                            // real answer, and the one worth reporting.
                            HttpReply::Message(_) => Self::tool_reply(first),
                            HttpReply::Refused(..) => Err(e),
                        };
                    }
                },
            }
        };

        tracing::info!(
            url,
            tool = tool_name,
            expired = sent.is_some(),
            "Backend requires an MCP session; retrying the call inside one"
        );

        let retried =
            Self::http_exchange(&client, url, Some(&session), &body, HTTP_TIMEOUT, "Backend")
                .await?;
        Self::tool_reply(retried)
    }

    /// Forget a backend's streamable-http session, and tell the server it is
    /// finished with.
    ///
    /// Returns at once. The slot may be locked by a call halfway through a
    /// handshake, and stopping a backend should not wait out someone else's
    /// thirty-second timeout; the session is ended when that lock comes free,
    /// including one that call opens after the backend was let go.
    pub async fn release_http_session(&self, backend_id: &Uuid) {
        let Some(slot) = self.http_sessions.write().await.remove(backend_id) else {
            return;
        };
        tokio::spawn(async move {
            if let Some(session) = slot.lock().await.take() {
                Self::end_http_session(session);
            }
        });
    }

    /// The slot a backend's session lives in, created empty on first use.
    async fn session_slot(&self, backend_id: Uuid) -> SessionSlot {
        if let Some(slot) = self.http_sessions.read().await.get(&backend_id) {
            return Arc::clone(slot);
        }
        Arc::clone(
            self.http_sessions
                .write()
                .await
                .entry(backend_id)
                .or_default(),
        )
    }

    /// The session to send with a request under `config`, if there is one.
    async fn session_for(slot: &SessionSlot, config: &serde_json::Value) -> Option<String> {
        slot.lock()
            .await
            .as_ref()
            .filter(|s| s.config == *config)
            .map(|s| s.id.clone())
    }

    async fn keep_http_session(&self, backend_id: Uuid, config: &serde_json::Value, id: String) {
        let slot = self.session_slot(backend_id).await;
        let previous = slot.lock().await.replace(HttpSession {
            config: config.clone(),
            id: id.clone(),
        });
        if let Some(previous) = previous.filter(|p| p.id != id) {
            Self::end_http_session(previous);
        }
    }

    /// `DELETE` a session, in the background and on a best-effort basis.
    ///
    /// The spec asks a client to do this for a session it no longer needs. A
    /// server that ignores it reclaims the session on its own timeout, so a
    /// failure here is not worth a caller waiting on, or hearing about.
    fn end_http_session(session: HttpSession) {
        let (Ok(url), Ok(client)) = (
            Self::config_url(&session.config).map(str::to_string),
            Self::build_http_client(&session.config),
        ) else {
            return;
        };
        tokio::spawn(async move {
            let _ = Self::with_session(client.delete(&url), Some(&session.id))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
        });
    }

    /// One JSON-RPC request to a streamable-http backend, and what came back.
    ///
    /// `what` names the request in a transport error.
    async fn http_exchange(
        client: &reqwest::Client,
        url: &str,
        session: Option<&str>,
        body: &serde_json::Value,
        timeout: std::time::Duration,
        what: &str,
    ) -> Result<HttpReply, String> {
        let resp = Self::with_session(client.post(url), session)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| format!("{} request failed: {}", what, e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Ok(HttpReply::Refused(status, text));
        }

        Self::read_streamable_response(resp, body.get("id").and_then(|id| id.as_u64()))
            .await
            .map(HttpReply::Message)
            .map_err(|e| format!("Failed to parse backend response: {}", e))
    }

    /// Whether a reply means "you are not in a session, and I need you to be".
    ///
    /// Without a session, the spec's `400` and `404` count, and so does a
    /// JSON-RPC error that says so in words: a stateful server refusing a
    /// session-less request cannot have run it, so replaying it is safe.
    ///
    /// With a session, `404` is the spec's "that session is gone". A `400` only
    /// counts when its body is about a session — the TypeScript SDK's README
    /// server answers a forgotten session that way — because a `400` for
    /// anything else (the Go SDK's "duplicate in-flight request ID", a proxy's
    /// objection) is not cured by a new session, and opening one would only
    /// orphan the old. The words on a `200` never count here: an error like
    /// that on a request already in a session is more likely the tool's own,
    /// and replaying a call that may have run is how it runs twice.
    fn refused_for_want_of_session(reply: &HttpReply, sent_session: bool) -> bool {
        match reply {
            HttpReply::Refused(status, body) => match *status {
                reqwest::StatusCode::NOT_FOUND => true,
                reqwest::StatusCode::BAD_REQUEST => {
                    !sent_session || Self::names_a_missing_session(body)
                }
                _ => false,
            },
            HttpReply::Message(json) => {
                !sent_session
                    && json
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .is_some_and(Self::names_a_missing_session)
            }
        }
    }

    /// Whether an error message is a server saying the request needed a session.
    ///
    /// The official SDKs, verbatim:
    ///
    /// * Go — `method "tools/call" is invalid during session initialization`,
    ///   with `200 OK`, which is the one that needs this at all.
    /// * Python — `Bad Request: Missing session ID`, `Session not found`.
    /// * TypeScript — `Bad Request: Server not initialized`,
    ///   `Bad Request: Mcp-Session-Id header is required`, `Session not found`,
    ///   and the README's `Bad Request: No valid session ID provided`.
    fn names_a_missing_session(message: &str) -> bool {
        let message = message.to_ascii_lowercase();
        let about_a_session = message.contains("session")
            && [
                "initializ",
                "missing",
                "required",
                "not found",
                "no valid",
                "invalid session",
                "unknown session",
                "expired",
            ]
            .iter()
            .any(|phrase| message.contains(phrase));
        about_a_session || message.contains("not initialized")
    }

    /// What a tool call's JSON-RPC reply amounts to.
    fn tool_reply(reply: HttpReply) -> Result<serde_json::Value, String> {
        match reply {
            HttpReply::Refused(status, body) => {
                Err(format!("Backend returned HTTP {}: {}", status, body))
            }
            HttpReply::Message(json) => {
                if let Some(result) = json.get("result") {
                    Ok(result.clone())
                } else if let Some(error) = json.get("error") {
                    Err(format!("Backend error: {}", error))
                } else {
                    Ok(json)
                }
            }
        }
    }

    /// The `Mcp-Session-Id` a streamable-http server assigned, if it assigned
    /// one. Absent on a stateless server, which is the common case.
    fn session_id(resp: &reqwest::Response) -> Option<String> {
        resp.headers()
            .get(MCP_SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    }

    /// Attach a session to a request, when there is one to attach.
    fn with_session(
        builder: reqwest::RequestBuilder,
        session: Option<&str>,
    ) -> reqwest::RequestBuilder {
        match session {
            Some(id) => builder.header(MCP_SESSION_HEADER, id),
            None => builder,
        }
    }

    /// `initialize` + `notifications/initialized`, returning the session the
    /// server opened, if it opened one.
    async fn http_handshake(client: &reqwest::Client, url: &str) -> Result<Option<String>, String> {
        let resp = client
            .post(url)
            .json(&Self::rpc("initialize", Self::initialize_params()))
            .timeout(HTTP_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("HTTP initialize request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Initialize returned HTTP {}: {}", status, body));
        }

        // Read before the body, because reading the body consumes the response.
        let session = Self::session_id(&resp);

        let init_json = Self::read_streamable_response(resp, None)
            .await
            .map_err(|e| format!("Failed to parse initialize response: {}", e))?;
        if let Some(error) = init_json.get("error") {
            return Err(format!("Initialize failed: {}", error));
        }
        tracing::info!(url, resp = ?init_json, "HTTP MCP initialize succeeded");

        // Fire and forget: a server that rejects the notification still works.
        let _ = Self::with_session(client.post(url), session.as_deref())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        Ok(session)
    }

    /// A handshake that must open a session.
    ///
    /// Errors when the server does not open one: reaching here means it already
    /// refused a session-less request, so an initialize that assigns nothing
    /// leaves no way to satisfy it, and saying so beats retrying into the same
    /// refusal.
    async fn open_http_session(client: &reqwest::Client, url: &str) -> Result<String, String> {
        Self::http_handshake(client, url).await?.ok_or_else(|| {
            format!(
                "Backend rejected a request without a session but its initialize \
                 response carried no {MCP_SESSION_HEADER} header"
            )
        })
    }

    // ── SSE (the 2024-11-05 HTTP+SSE transport) ─────────────────────────

    /// Discover tools from an SSE MCP backend: open the stream, wait for the
    /// `endpoint` it announces, and run the handshake and `tools/list` through
    /// it.
    pub async fn discover_sse_tools(
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Vec<DiscoveredTool>, String> {
        let url = Self::config_url(config)?;
        let client = Self::build_http_client(config)?;

        tracing::info!(backend = name, url, "Discovering tools from SSE backend");

        let mut sse = SseConnection::open(&client, url).await?;
        let tools_json = sse
            .request(&client, "tools/list", serde_json::json!({}))
            .await?;

        let tools = Self::parse_tool_list(name, &tools_json, "sse");
        tracing::info!(
            backend = name,
            tool_count = tools.len(),
            "SSE backend tools discovered"
        );
        Ok(tools)
    }

    /// Forward a tool call to an SSE MCP backend.
    pub async fn call_sse_tool(
        config: &serde_json::Value,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let url = Self::config_url(config)?;
        let client = Self::build_http_client(config)?;

        let mut sse = SseConnection::open(&client, url).await?;
        let reply = sse
            .request(
                &client,
                "tools/call",
                serde_json::json!({ "name": tool_name, "arguments": arguments }),
            )
            .await?;
        Self::tool_reply(HttpReply::Message(reply))
    }

    // ── Shared by both HTTP transports ──────────────────────────────────

    fn config_url(config: &serde_json::Value) -> Result<&str, String> {
        config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Backend config missing 'url' field".to_string())
    }

    fn initialize_params() -> serde_json::Value {
        serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "mcp-gateway", "version": env!("CARGO_PKG_VERSION") }
        })
    }

    /// A JSON-RPC request with an id no other request from this process has.
    ///
    /// Every request used to carry `1`, which was harmless while each call ran
    /// in a session of its own. Calls to a stateful backend now share one, and
    /// the SDKs route a reply by its request's id within a session: two
    /// concurrent calls with the same id meant the Python and TypeScript
    /// servers handed the second caller the first caller's result, and the Go
    /// server refused the second outright.
    fn rpc(method: &str, params: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            "method": method,
            "params": params,
        })
    }

    /// The tools out of a `tools/list` reply: a JSON-RPC envelope, or a bare
    /// result.
    fn parse_tool_list(
        backend: &str,
        reply: &serde_json::Value,
        transport: &str,
    ) -> Vec<DiscoveredTool> {
        let result = reply.get("result").unwrap_or(reply);
        let Some(tools) = result.get("tools").and_then(|t| t.as_array()) else {
            tracing::warn!(
                backend,
                transport,
                error = ?reply.get("error"),
                "tools/list returned no tools array"
            );
            return vec![];
        };
        tools
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
            .collect()
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
        id: Option<u64>,
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
            return Self::parse_sse_body(&body, id)
                .ok_or_else(|| format!("No JSON-RPC message in SSE response: {}", body));
        }

        // application/json (or unlabelled). Fall back to SSE framing if the body
        // isn't valid JSON but looks like an event stream.
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => Ok(v),
            Err(e) => Self::parse_sse_body(&body, id)
                .ok_or_else(|| format!("Failed to parse response body: {} (body: {})", e, body)),
        }
    }

    /// Extract the JSON-RPC payload from the events of an SSE body: the reply
    /// to request `id` if it is there, then any reply (a `result` or `error`),
    /// then whatever else parsed — a server may interleave progress
    /// notifications and its own requests ahead of the answer.
    fn parse_sse_body(body: &str, id: Option<u64>) -> Option<serde_json::Value> {
        let mut decoder = SseDecoder::default();
        let mut events = decoder.feed(body.as_bytes());
        events.extend(decoder.finish());

        let id = id.map(|id| serde_json::json!(id));
        let mut reply: Option<serde_json::Value> = None;
        let mut fallback: Option<serde_json::Value> = None;
        for event in events {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&event.data) {
                if val.get("result").is_some() || val.get("error").is_some() {
                    if id.is_some() && val.get("id") == id.as_ref() {
                        return Some(val);
                    }
                    reply.get_or_insert(val);
                } else {
                    fallback.get_or_insert(val);
                }
            }
        }
        reply.or(fallback)
    }

    fn build_http_client(config: &serde_json::Value) -> Result<reqwest::Client, String> {
        // reqwest defaults to no timeout at all. Every JSON-RPC call sets its
        // own per-request deadline, but the SSE stream cannot — a per-request
        // timeout would kill the long-lived stream it is opening — so the
        // connect deadline lives on the client, where it covers every path.
        // A backend that completes the TCP handshake and then never answers
        // used to hang the caller forever, and on startup that meant the
        // listener never bound.
        let mut builder = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .redirect(Self::same_origin_redirects());

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

    /// Follow a redirect only while it stays on the origin it started from.
    ///
    /// Every request carries the backend's configured headers as client-wide
    /// defaults. reqwest strips `Authorization` and `Cookie` when a redirect
    /// leaves the host, and nothing else — so a backend (or anything on its
    /// path) answering `307` towards another host received an `X-API-Key` or
    /// any other credential header verbatim, which is the exfiltration the SSE
    /// endpoint check exists to stop, by a different door. A backend that
    /// genuinely moved is re-registered at its new address.
    fn same_origin_redirects() -> reqwest::redirect::Policy {
        reqwest::redirect::Policy::custom(|attempt| {
            let same_origin = attempt
                .previous()
                .first()
                .is_some_and(|first| first.origin() == attempt.url().origin());
            if attempt.previous().len() > 10 {
                attempt.error("too many redirects")
            } else if same_origin {
                attempt.follow()
            } else {
                attempt.stop()
            }
        })
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

    /// Resolve the `endpoint` an SSE backend announced against the URL its
    /// stream was opened on — RFC 3986 resolution, which is what the official
    /// SDK clients do (`new URL(data, sseUrl)` in TypeScript, `urljoin` in
    /// Python) — and refuse the result unless it is on that same origin.
    ///
    /// The announcement comes from the (untrusted) backend, and the gateway
    /// POSTs to it carrying the stored Authorization header (a client-wide
    /// default header). Only a same-origin endpoint is accepted, so a malicious
    /// or compromised backend can't redirect that credentialed POST to an
    /// attacker host (token exfiltration) or an internal SSRF target like
    /// http://169.254.169.254/. The check runs on the *resolved* URL, which is
    /// what closes the forms that are not visibly absolute: a protocol-relative
    /// `//attacker.evil/` and a userinfo `https://vendor@attacker.evil/` both
    /// resolve off-origin and are refused like any other.
    fn resolve_sse_url(base_url: &str, endpoint: &str) -> Result<String, String> {
        let base = reqwest::Url::parse(base_url)
            .map_err(|e| format!("Backend URL '{}' is not a valid URL: {}", base_url, e))?;
        let resolved = base.join(endpoint).map_err(|e| {
            format!(
                "SSE backend announced an endpoint that is not a URL ('{}'): {}",
                endpoint, e
            )
        })?;
        if resolved.origin() != base.origin() {
            return Err(format!(
                "SSE backend returned a cross-origin endpoint URL ('{}'); refusing to send \
                 the backend's credentials off-origin",
                endpoint
            ));
        }
        Ok(resolved.into())
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

/// JSON-RPC request ids, unique for the life of the process. See
/// [`BackendManager::rpc`].
static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// How long one streamable-http request, or an SSE handshake step, may take.
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Covers the wait for the SSE stream's response headers only: a timeout on the
/// request would apply to the whole stream, which is meant to stay open.
const SSE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SSE_ENDPOINT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SSE_POST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a streamable-http POST came back with.
enum HttpReply {
    /// A 2xx, and the JSON-RPC message it carried.
    Message(serde_json::Value),
    /// Anything else, and the body that came with it.
    Refused(reqwest::StatusCode, String),
}

/// A session a streamable-http backend opened, and the configuration it was
/// opened under.
///
/// The configuration is part of the key because a session belongs to the
/// address and credentials it was opened with: a backend edited to point
/// somewhere else, or to carry a different token, starts again rather than
/// presenting its predecessor's session.
struct HttpSession {
    config: serde_json::Value,
    id: String,
}

type SessionSlot = Arc<Mutex<Option<HttpSession>>>;

/// A live HTTP+SSE session: the GET stream the backend answers on, and the
/// endpoint it told us to POST to.
///
/// Dropping it ends the stream. The reader task is aborted rather than left to
/// notice by itself, because it only noticed when it next handed an event on,
/// and a stream carrying nothing but keep-alive comments — the Python SDK sends
/// one every 15 seconds and nothing else — never gave it one. Every tool call
/// left its GET open behind it for as long as the backend stayed up.
struct SseConnection {
    post_url: String,
    rx: tokio::sync::mpsc::Receiver<serde_json::Value>,
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for SseConnection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl SseConnection {
    /// Open the stream, resolve the endpoint it announces, and complete the
    /// MCP handshake over it.
    async fn open(client: &reqwest::Client, url: &str) -> Result<Self, String> {
        let response = tokio::time::timeout(
            SSE_CONNECT_TIMEOUT,
            client
                .get(url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send(),
        )
        .await
        .map_err(|_| "SSE GET timed out waiting for response headers".to_string())?
        .map_err(|e| format!("SSE GET connect failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // What a streamable-http server says to a bare GET: 405 from the
            // Python SDK's FastMCP router, 400 from the Go and Python SDKs'
            // stateful handlers, 406 when it wants a different Accept.
            let hint = if matches!(status.as_u16(), 400 | 405 | 406) {
                " (a streamable-http server refuses a GET without a session; \
                 if this is one, register it with that transport)"
            } else {
                ""
            };
            return Err(format!(
                "SSE GET returned HTTP {}: {}{}",
                status, body, hint
            ));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel::<String>();
        let (json_tx, json_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(32);
        let mut connection = Self {
            post_url: String::new(),
            rx: json_rx,
            reader: tokio::spawn(Self::read_stream(response, endpoint_tx, json_tx)),
        };

        let announced = match tokio::time::timeout(SSE_ENDPOINT_TIMEOUT, endpoint_rx).await {
            Ok(Ok(endpoint)) => endpoint,
            Ok(Err(_)) => return Err("SSE stream closed before sending endpoint event".into()),
            Err(_) if !content_type.contains("text/event-stream") => {
                return Err(format!(
                    "Timeout (15s) waiting for SSE endpoint event: the backend answered with \
                     Content-Type '{}', not an event stream. If it is a streamable-http \
                     server, register it with that transport",
                    content_type
                ));
            }
            Err(_) => return Err("Timeout (15s) waiting for SSE endpoint event".into()),
        };

        connection.post_url = BackendManager::resolve_sse_url(url, announced.trim())?;
        tracing::info!(post_url = %connection.post_url, "SSE endpoint discovered");

        let init = connection
            .request(client, "initialize", BackendManager::initialize_params())
            .await?;
        if let Some(error) = init.get("error") {
            return Err(format!("SSE initialize failed: {}", error));
        }
        tracing::debug!(resp = ?init, "SSE MCP initialize succeeded");

        // Fire and forget: a server that rejects the notification still works.
        let _ = client
            .post(&connection.post_url)
            .json(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        Ok(connection)
    }

    /// Pump the stream: the first `endpoint` event to the handshake, every
    /// `message` event that parses to the reply channel.
    async fn read_stream(
        mut response: reqwest::Response,
        endpoint_tx: tokio::sync::oneshot::Sender<String>,
        json_tx: tokio::sync::mpsc::Sender<serde_json::Value>,
    ) {
        let mut decoder = SseDecoder::default();
        let mut endpoint_tx = Some(endpoint_tx);
        while let Ok(Some(chunk)) = response.chunk().await {
            for event in decoder.feed(&chunk) {
                match event.event_type.as_str() {
                    "endpoint" => {
                        if let Some(tx) = endpoint_tx.take() {
                            let _ = tx.send(event.data);
                        }
                    }
                    "message" => {
                        if let Ok(json) = serde_json::from_str(&event.data) {
                            if json_tx.send(json).await.is_err() {
                                return;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// POST one request to the announced endpoint and wait on the stream for
    /// its reply.
    async fn request(
        &mut self,
        client: &reqwest::Client,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let request = BackendManager::rpc(method, params);
        let id = request["id"].as_u64().unwrap_or_default();
        let resp = client
            .post(&self.post_url)
            .json(&request)
            .timeout(SSE_POST_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("SSE POST {} failed: {}", method, e))?;

        // The reply travels on the stream; the POST itself is only
        // acknowledged, normally with 202. Without this check a refused POST —
        // an expired session, a mistyped path — surfaced thirty seconds later
        // as a timeout on the stream, with the reason nowhere in it.
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "SSE POST {} returned HTTP {}: {}",
                method, status, body
            ));
        }

        self.reply_to(id, method).await
    }

    /// The reply to request `id`, and nothing else.
    ///
    /// A notification has no id, and a request the *server* sends — a `ping`,
    /// a `roots/list` — has an id and a method. Taking the first message with
    /// any id at all, as this used to, handed the caller whichever of those
    /// happened to arrive first.
    async fn reply_to(&mut self, id: u64, method: &str) -> Result<serde_json::Value, String> {
        let deadline = tokio::time::Instant::now() + HTTP_TIMEOUT;
        let id = serde_json::json!(id);
        loop {
            match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                Ok(Some(json)) if json.get("id") == Some(&id) && json.get("method").is_none() => {
                    return Ok(json);
                }
                Ok(Some(_)) => continue,
                Ok(None) => {
                    return Err(format!(
                        "SSE stream closed while waiting for the {} response",
                        method
                    ));
                }
                Err(_) => {
                    return Err(format!(
                        "Timeout (30s) waiting for the SSE {} response",
                        method
                    ));
                }
            }
        }
    }
}

/// One dispatched Server-Sent Event.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SseEvent {
    /// `message` when the stream named no type, as the spec defines.
    event_type: String,
    data: String,
}

/// An incremental `text/event-stream` decoder, following the WHATWG "event
/// stream interpretation".
///
/// This replaces a split on `"\n\n"`, which is wrong in the way that matters
/// most. The spec lets a line end in CRLF, LF or a lone CR, and sse-starlette,
/// which the official Python MCP SDK streams through, ends every line in CRLF.
/// `"\r\n\r\n"` contains no `"\n\n"`, so no event ever dispatched: the
/// `endpoint` event sat in the buffer, the handshake timed out after fifteen
/// seconds, and no SSE server built on the Python SDK could be registered at
/// all (#12).
///
/// It works on bytes and decodes only whole lines, so a multi-byte character
/// split across two network reads survives. Decoding each chunk on its own
/// turned the halves into two U+FFFDs.
#[derive(Default)]
struct SseDecoder {
    line: Vec<u8>,
    /// The last byte seen was a CR. An LF straight after it finishes the same
    /// line ending — even when the two arrive in different reads — rather than
    /// ending an empty line of its own, which would dispatch the event early.
    after_cr: bool,
    event_type: String,
    data: String,
}

impl SseDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for &byte in bytes {
            if std::mem::take(&mut self.after_cr) && byte == b'\n' {
                continue;
            }
            match byte {
                b'\r' => {
                    self.after_cr = true;
                    self.end_line(&mut events);
                }
                b'\n' => self.end_line(&mut events),
                _ => self.line.push(byte),
            }
        }
        events
    }

    /// The end of a body.
    ///
    /// The spec discards an event the stream never closed with a blank line.
    /// A one-shot POST response is sometimes written without that last blank
    /// line, and discarding there would turn a good reply into "no JSON-RPC
    /// message", so a finished body dispatches what it holds.
    fn finish(&mut self) -> Option<SseEvent> {
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.field(&line);
        }
        self.dispatch()
    }

    fn end_line(&mut self, events: &mut Vec<SseEvent>) {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            events.extend(self.dispatch());
        } else {
            self.field(&line);
        }
    }

    fn field(&mut self, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        if line.starts_with(':') {
            return; // a comment, e.g. a keep-alive
        }
        let (name, value) = match line.split_once(':') {
            // One space after the colon is part of the syntax, not the value.
            Some((name, value)) => (name, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_ref(), ""),
        };
        match name {
            "event" => self.event_type = value.to_string(),
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            // `id`, `retry`, and anything unrecognised.
            _ => {}
        }
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event_type = std::mem::take(&mut self.event_type);
        let mut data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return None;
        }
        data.pop(); // the LF after the last data line
        Some(SseEvent {
            event_type: if event_type.is_empty() {
                "message".to_string()
            } else {
                event_type
            },
            data,
        })
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

    /// Exactly what the Python SDK's `SseServerTransport("/messages/")`
    /// announces: a path with a trailing slash and the session in the query.
    #[test]
    fn the_python_sdk_endpoint_resolves_with_its_query_intact() {
        let out = BackendManager::resolve_sse_url(
            "http://192.168.116.48:18080/sse",
            "/messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a",
        );
        assert_eq!(
            out.unwrap(),
            "http://192.168.116.48:18080/messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a"
        );
    }

    /// A path with no leading slash is relative to the stream's own directory,
    /// which is where the SDK clients resolve it. Gluing it to the origin put a
    /// server mounted under a prefix at the wrong path.
    #[test]
    fn a_relative_path_resolves_against_the_stream_url() {
        let out = BackendManager::resolve_sse_url("https://h.example/mcp/sse", "messages?s=1");
        assert_eq!(out.unwrap(), "https://h.example/mcp/messages?s=1");
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

    /// Neither of these starts with a scheme, so a check that only inspected
    /// "absolute" endpoints let both through as if they were paths.
    #[test]
    fn endpoints_that_only_resolve_off_origin_are_rejected() {
        for endpoint in [
            "//attacker.evil/collect",
            "https://tools.vendor.com@attacker.evil/collect",
            "https://tools.vendor.com.attacker.evil/collect",
            "https://tools.vendor.com:8443/messages",
        ] {
            let out = BackendManager::resolve_sse_url("https://tools.vendor.com/sse", endpoint);
            assert!(out.is_err(), "{endpoint} must be rejected: {out:?}");
        }
    }

    #[test]
    fn an_explicit_default_port_is_the_same_origin() {
        let out = BackendManager::resolve_sse_url(
            "https://tools.vendor.com/sse",
            "https://tools.vendor.com:443/messages",
        );
        assert!(out.is_ok(), "{out:?}");
    }

    #[test]
    fn sse_body_extracts_jsonrpc_result_frame() {
        // n8n answers a streamable-http POST with a single SSE `message` event
        // carrying the JSON-RPC reply.
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n";
        let out = BackendManager::parse_sse_body(body, None).expect("should extract frame");
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
        let out = BackendManager::parse_sse_body(body, None).expect("should extract result frame");
        assert_eq!(out["result"]["ok"], true);
    }

    /// The Python SDK's streamable-http responses end every line in CRLF. With
    /// one event that happened to parse; with a notification ahead of the
    /// result, the two frames were one frame and neither was JSON.
    #[test]
    fn a_crlf_body_with_a_notification_first_still_yields_the_result() {
        let body = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{}}\r\n\r\n\
                    event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"ok\":true}}\r\n\r\n";
        let out = BackendManager::parse_sse_body(body, None).expect("should extract result frame");
        assert_eq!(out["result"]["ok"], true);
    }

    /// A reply to another request on the same stream is not this request's.
    #[test]
    fn sse_body_prefers_the_reply_to_this_request() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"who\":\"other\"}}\n\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":8,\"result\":{\"who\":\"me\"}}\n\n";
        let out = BackendManager::parse_sse_body(body, Some(8)).unwrap();
        assert_eq!(out["result"]["who"], "me");
    }

    #[test]
    fn request_ids_are_never_reused() {
        let a = BackendManager::rpc("tools/call", serde_json::json!({}));
        let b = BackendManager::rpc("tools/call", serde_json::json!({}));
        assert_ne!(a["id"], b["id"]);
    }

    #[test]
    fn sse_body_returns_error_frame() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"nope\"}}\n\n";
        let out = BackendManager::parse_sse_body(body, None).expect("should extract error frame");
        assert_eq!(out["error"]["code"], -32000);
    }

    #[test]
    fn sse_body_with_no_json_returns_none() {
        assert!(BackendManager::parse_sse_body(": keep-alive comment\n\n", None).is_none());
    }

    #[test]
    fn a_body_without_its_final_blank_line_still_yields_its_event() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
        assert!(BackendManager::parse_sse_body(body, None).is_some());
    }

    // ── The SSE decoder (issue #12) ─────────────────────────────────────

    fn decode_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(decoder.feed(chunk));
        }
        events
    }

    fn event(event_type: &str, data: &str) -> SseEvent {
        SseEvent {
            event_type: event_type.into(),
            data: data.into(),
        }
    }

    /// The bytes the Python SDK's SSE transport actually sends, captured from
    /// `mcp` 1.30.0 behind uvicorn — the stream the reporter's backend served.
    #[test]
    fn the_python_sdk_endpoint_event_dispatches() {
        let stream = b"event: endpoint\r\ndata: /messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a\r\n\r\n";
        assert_eq!(
            decode_all(&[stream]),
            vec![event(
                "endpoint",
                "/messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a"
            )]
        );
    }

    #[test]
    fn every_line_ending_the_spec_allows_dispatches() {
        for stream in [
            &b"event: endpoint\ndata: /m\n\n"[..],
            &b"event: endpoint\r\ndata: /m\r\n\r\n"[..],
            &b"event: endpoint\rdata: /m\r\r"[..],
        ] {
            assert_eq!(
                decode_all(&[stream]),
                vec![event("endpoint", "/m")],
                "{:?}",
                String::from_utf8_lossy(stream)
            );
        }
    }

    /// A CR at the end of one read and its LF at the start of the next are one
    /// line ending. Read as two, the LF would end an empty line and dispatch an
    /// event before its data had arrived.
    #[test]
    fn a_crlf_split_across_two_reads_is_one_line_ending() {
        assert_eq!(
            decode_all(&[b"event: endpoint\r", b"\ndata: /m\r", b"\n\r", b"\n"]),
            vec![event("endpoint", "/m")]
        );
    }

    #[test]
    fn an_event_split_byte_by_byte_still_arrives_whole() {
        let stream = b"event: message\r\ndata: {\"id\":1}\r\n\r\n";
        let chunks: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(decode_all(&chunks), vec![event("message", "{\"id\":1}")]);
    }

    #[test]
    fn a_character_split_across_reads_is_not_mangled() {
        let stream = "data: {\"text\":\"café ✓\"}\n\n".as_bytes();
        let split = stream.iter().position(|&b| b == 0xE2).unwrap() + 1;
        let (a, b) = stream.split_at(split);
        assert_eq!(
            decode_all(&[a, b]),
            vec![event("message", "{\"text\":\"café ✓\"}")]
        );
    }

    #[test]
    fn keep_alive_comments_and_unknown_fields_dispatch_nothing() {
        assert!(decode_all(&[
            b": ping - 2026-09-16 18:10:24.471305+00:00\r\n\r\n",
            b"retry: 3000\nid: 7\n\n",
        ])
        .is_empty());
    }

    #[test]
    fn data_lines_join_with_a_newline_and_one_leading_space_is_syntax() {
        assert_eq!(
            decode_all(&[b"data:a\ndata:  b\ndata\n\n"]),
            vec![event("message", "a\n b\n")]
        );
    }

    #[test]
    fn the_event_type_does_not_leak_into_the_next_event() {
        assert_eq!(
            decode_all(&[b"event: endpoint\ndata: /m\n\ndata: {}\n\n"]),
            vec![event("endpoint", "/m"), event("message", "{}")]
        );
    }

    // ── Streamable-http sessions (issues #10, #13) ──────────────────────

    fn header_of(builder: reqwest::RequestBuilder) -> Option<String> {
        let request = builder.build().expect("request builds");
        request
            .headers()
            .get(MCP_SESSION_HEADER)
            .map(|v| v.to_str().unwrap().to_string())
    }

    /// The bug: `initialize` opened a session and the follow-up requests were
    /// sent without it, so a session-strict backend treated `tools/list` as
    /// un-initialized and answered with an empty tool list.
    #[test]
    fn a_session_travels_on_the_requests_that_follow_initialize() {
        let client = reqwest::Client::new();
        assert_eq!(
            header_of(BackendManager::with_session(
                client.post("http://backend.local/mcp"),
                Some("sess-abc123"),
            )),
            Some("sess-abc123".to_string())
        );
    }

    /// The common case is a stateless server, which assigns no session; the
    /// header must not be invented for one.
    #[test]
    fn a_stateless_backend_gets_no_session_header() {
        let client = reqwest::Client::new();
        assert_eq!(
            header_of(BackendManager::with_session(
                client.post("http://backend.local/mcp"),
                None
            )),
            None
        );
    }

    fn refused(status: u16) -> HttpReply {
        refused_with(status, "")
    }

    fn refused_with(status: u16, body: &str) -> HttpReply {
        HttpReply::Refused(reqwest::StatusCode::from_u16(status).unwrap(), body.into())
    }

    fn rpc_error(message: &str) -> HttpReply {
        HttpReply::Message(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "error": { "code": 0, "message": message }
        }))
    }

    /// Only the two statuses the spec gives a session meaning buy a retry. A
    /// 500 is the backend's own failure and retrying it inside a session would
    /// just double the load; a 401 is a credential, not a session.
    #[test]
    fn only_the_session_statuses_trigger_a_handshake() {
        for status in [400, 404] {
            assert!(BackendManager::refused_for_want_of_session(
                &refused(status),
                false
            ));
        }
        // In a session, 404 is "that session is gone".
        assert!(BackendManager::refused_for_want_of_session(
            &refused(404),
            true
        ));
        for status in [401, 403, 405, 406, 500, 502] {
            assert!(!BackendManager::refused_for_want_of_session(
                &refused(status),
                false
            ));
        }
    }

    /// In a session, a 400 is only about the session if it says so. The
    /// TypeScript README server answers a forgotten session with one; the Go
    /// SDK answers a duplicate request id with another, which a new session
    /// does not cure.
    #[test]
    fn in_a_session_a_400_counts_only_when_it_is_about_the_session() {
        assert!(BackendManager::refused_for_want_of_session(
            &refused_with(
                400,
                r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Bad Request: No valid session ID provided"},"id":null}"#
            ),
            true
        ));
        assert!(!BackendManager::refused_for_want_of_session(
            &refused_with(400, "duplicate in-flight request ID 1\n"),
            true
        ));
        assert!(!BackendManager::refused_for_want_of_session(
            &refused(400),
            true
        ));
    }

    /// Issue #13: the Go SDK refuses with `200 OK` and says why in the body.
    #[test]
    fn the_go_sdk_refusal_in_a_200_triggers_a_handshake() {
        let go = rpc_error("method \"tools/call\" is invalid during session initialization");
        assert!(BackendManager::refused_for_want_of_session(&go, false));
    }

    /// A successful reply, and a tool's own error, are answers.
    #[test]
    fn a_result_or_an_unrelated_error_is_an_answer() {
        let ok = HttpReply::Message(serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}}));
        assert!(!BackendManager::refused_for_want_of_session(&ok, false));
        assert!(!BackendManager::refused_for_want_of_session(
            &rpc_error("Invalid params: 'path' is required"),
            false
        ));
    }

    /// In a session, words are not enough. The request was accepted into a
    /// session, so an error that mentions one is more likely the tool's —
    /// and replaying a tool call that may have run is how it runs twice.
    #[test]
    fn inside_a_session_only_a_status_triggers_a_handshake() {
        assert!(!BackendManager::refused_for_want_of_session(
            &rpc_error("Session not found"),
            true
        ));
    }

    /// Every refusal the official SDKs send, verbatim.
    #[test]
    fn the_sdks_refusals_are_all_recognised() {
        for message in [
            "method \"tools/call\" is invalid during session initialization",
            "method \"tools/list\" is invalid during session initialization",
            "Bad Request: Missing session ID",
            "Session not found",
            "Bad Request: Server not initialized",
            "Bad Request: Mcp-Session-Id header is required",
            "Bad Request: No valid session ID provided",
        ] {
            assert!(
                BackendManager::names_a_missing_session(message),
                "{message}"
            );
        }
        for message in [
            "Invalid params",
            "Tool not found: frobnicate",
            "Internal error",
            "The session with the printer was reset",
        ] {
            assert!(
                !BackendManager::names_a_missing_session(message),
                "{message}"
            );
        }
    }

    // ── Against a live socket ───────────────────────────────────────────
    //
    // A handful of lines of HTTP/1.1 over a raw listener, rather than a
    // framework, because what these tests are about is the exact bytes: CRLF
    // line endings, a stream held open, a body with no length.

    mod wire {
        use std::collections::HashMap;
        use std::future::Future;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        pub struct Request {
            pub method: String,
            pub path: String,
            pub headers: HashMap<String, String>,
            pub body: serde_json::Value,
        }

        /// Serve every connection with `handler`, which writes its own response
        /// and may hold the stream open. Returns the base URL.
        pub async fn serve<F, Fut>(handler: F) -> String
        where
            F: Fn(Request, TcpStream) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = ()> + Send + 'static,
        {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let handler = std::sync::Arc::new(handler);
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        if let Some(request) = read_request(&mut stream).await {
                            handler(request, stream).await;
                        }
                    });
                }
            });
            base
        }

        async fn read_request(stream: &mut TcpStream) -> Option<Request> {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let head_end = loop {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i;
                }
                let n = stream.read(&mut chunk).await.ok()?;
                if n == 0 {
                    return None;
                }
                buf.extend_from_slice(&chunk[..n]);
            };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let mut lines = head.split("\r\n");
            let mut request_line = lines.next()?.split(' ');
            let method = request_line.next()?.to_string();
            let path = request_line.next()?.to_string();
            let headers: HashMap<String, String> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .collect();
            let length: usize = headers
                .get("content-length")
                .and_then(|l| l.parse().ok())
                .unwrap_or(0);
            let mut body = buf[head_end + 4..].to_vec();
            while body.len() < length {
                let n = stream.read(&mut chunk).await.ok()?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            Some(Request {
                method,
                path,
                headers,
                body: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
            })
        }

        pub async fn respond(
            mut stream: TcpStream,
            status: &str,
            headers: &[(&str, &str)],
            body: &str,
        ) {
            let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(body.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Counts {
        initialize: AtomicUsize,
        tool_calls: AtomicUsize,
        deletes: AtomicUsize,
        open_streams: AtomicUsize,
    }

    fn config_for(url: String) -> serde_json::Value {
        serde_json::json!({ "url": url })
    }

    async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
        for _ in 0..200 {
            if check() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    /// A stateful streamable-http server that behaves the way go-sdk v1.8.0
    /// was observed to: a session-less method gets `200 OK` and a code-0
    /// JSON-RPC error in an SSE body, an unknown session gets a plain-text 404,
    /// and `DELETE` ends a session.
    async fn go_sdk_server() -> (String, Arc<Counts>, Arc<std::sync::Mutex<Vec<String>>>) {
        let counts = Arc::new(Counts::default());
        let sessions = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        // (session, request id) pairs being answered right now.
        type InFlight = std::sync::Mutex<std::collections::HashSet<(String, String)>>;
        let in_flight: Arc<InFlight> = Arc::default();
        let (c, s, f) = (counts.clone(), sessions.clone(), in_flight.clone());
        let base = wire::serve(move |req, stream| {
            let (counts, sessions, in_flight) = (c.clone(), s.clone(), f.clone());
            async move {
                let session = req.headers.get("mcp-session-id").cloned();
                let sse = |json: serde_json::Value| format!("event: message\ndata: {json}\n\n");
                if req.method == "DELETE" {
                    counts.deletes.fetch_add(1, Ordering::SeqCst);
                    sessions
                        .lock()
                        .unwrap()
                        .retain(|s| Some(s) != session.as_ref());
                    return wire::respond(stream, "204 No Content", &[], "").await;
                }
                let method = req.body["method"].as_str().unwrap_or("").to_string();
                let id = req.body["id"].clone();
                if method == "initialize" {
                    let n = counts.initialize.fetch_add(1, Ordering::SeqCst);
                    let new = format!("SESSION{n}");
                    sessions.lock().unwrap().push(new.clone());
                    let body = sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "go", "version": "1"}
                    }}));
                    return wire::respond(
                        stream,
                        "200 OK",
                        &[("Content-Type", "text/event-stream"), ("Mcp-Session-Id", &new)],
                        &body,
                    )
                    .await;
                }
                if method.starts_with("notifications/") {
                    return wire::respond(stream, "202 Accepted", &[], "").await;
                }
                match session {
                    None => {
                        let body = sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": 0,
                            "message": format!("method \"{method}\" is invalid during session initialization")
                        }}));
                        wire::respond(stream, "200 OK", &[("Content-Type", "text/event-stream")], &body).await
                    }
                    Some(s) if !sessions.lock().unwrap().contains(&s) => {
                        wire::respond(
                            stream,
                            "404 Not Found",
                            &[("Content-Type", "text/plain; charset=utf-8")],
                            "session not found\n",
                        )
                        .await
                    }
                    Some(s) => {
                        // go-sdk refuses a request id already in flight in the
                        // same session.
                        let key = (s, id.to_string());
                        if !in_flight.lock().unwrap().insert(key.clone()) {
                            return wire::respond(
                                stream,
                                "400 Bad Request",
                                &[("Content-Type", "text/plain; charset=utf-8")],
                                &format!("duplicate in-flight request ID {id}\n"),
                            )
                            .await;
                        }
                        // Slow enough for concurrent calls to overlap.
                        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                        in_flight.lock().unwrap().remove(&key);
                        if method == "tools/call" {
                            counts.tool_calls.fetch_add(1, Ordering::SeqCst);
                        }
                        let text = req.body["params"]["arguments"]["text"].clone();
                        let body = sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                            "content": [{"type": "text", "text": text}]
                        }}));
                        wire::respond(stream, "200 OK", &[("Content-Type", "text/event-stream")], &body).await
                    }
                }
            }
        })
        .await;
        (format!("{base}/mcp"), counts, sessions)
    }

    fn echo(text: &str) -> serde_json::Value {
        serde_json::json!({ "text": text })
    }

    /// Issue #13, end to end: the call that used to fail on every attempt
    /// succeeds, and the session is opened once — not once per call.
    #[tokio::test]
    async fn a_go_sdk_backend_is_called_inside_one_session() {
        let (url, counts, _) = go_sdk_server().await;
        let manager = BackendManager::new();
        let id = Uuid::new_v4();
        let config = config_for(url);

        for text in ["one", "two", "three"] {
            let result = manager
                .call_http_tool(id, &config, "echo", &echo(text))
                .await
                .unwrap_or_else(|e| panic!("call failed: {e}"));
            assert_eq!(result["content"][0]["text"], text);
        }
        assert_eq!(counts.initialize.load(Ordering::SeqCst), 1);
        assert_eq!(counts.tool_calls.load(Ordering::SeqCst), 3);
    }

    /// Calls to one stateful backend share its session, so they must not share
    /// a request id. With the same id on every call, the Python and TypeScript
    /// servers hand a reply to whichever caller registered the id last, and
    /// the Go server refuses the second call outright.
    #[tokio::test]
    async fn concurrent_calls_in_one_session_each_get_their_own_reply() {
        let (url, counts, _) = go_sdk_server().await;
        let manager = Arc::new(BackendManager::new());
        let id = Uuid::new_v4();
        let config = config_for(url);
        manager
            .discover_http_tools(id, "go", &config)
            .await
            .unwrap();

        let calls: Vec<_> = (0..12)
            .map(|n| {
                let (manager, config) = (manager.clone(), config.clone());
                tokio::spawn(async move {
                    let text = format!("call {n}");
                    let result = manager
                        .call_http_tool(id, &config, "echo", &echo(&text))
                        .await
                        .unwrap_or_else(|e| panic!("{text} failed: {e}"));
                    assert_eq!(
                        result["content"][0]["text"], text,
                        "a reply went to the wrong caller"
                    );
                })
            })
            .collect();
        for call in calls {
            call.await.unwrap();
        }
        assert_eq!(counts.tool_calls.load(Ordering::SeqCst), 12);
        assert_eq!(
            counts.initialize.load(Ordering::SeqCst),
            1,
            "no call reopened the session"
        );
    }

    /// The connectivity probe runs its own handshake and ends its own session;
    /// the one calls are using is not touched.
    #[tokio::test]
    async fn a_probe_leaves_the_calls_session_alone() {
        let (url, counts, _) = go_sdk_server().await;
        let manager = BackendManager::new();
        let id = Uuid::new_v4();
        let config = config_for(url);
        manager
            .discover_http_tools(id, "go", &config)
            .await
            .unwrap();

        BackendManager::probe_http_tools("go", &config)
            .await
            .unwrap();
        eventually("the probe's session to be deleted", || {
            counts.deletes.load(Ordering::SeqCst) == 1
        })
        .await;

        manager
            .call_http_tool(id, &config, "echo", &echo("still here"))
            .await
            .unwrap();
        assert_eq!(
            counts.initialize.load(Ordering::SeqCst),
            2,
            "discovery and the probe only"
        );
    }

    /// A session the server has forgotten — a restart, an idle timeout — is
    /// answered 404, and the next call opens a fresh one without failing.
    #[tokio::test]
    async fn an_expired_session_is_replaced_without_failing_the_call() {
        let (url, counts, sessions) = go_sdk_server().await;
        let manager = BackendManager::new();
        let id = Uuid::new_v4();
        let config = config_for(url);

        manager
            .call_http_tool(id, &config, "echo", &echo("before"))
            .await
            .unwrap();
        sessions.lock().unwrap().clear();

        let result = manager
            .call_http_tool(id, &config, "echo", &echo("after"))
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "after");
        assert_eq!(counts.initialize.load(Ordering::SeqCst), 2);
    }

    /// Discovery's session is the one calls use, and letting a backend go
    /// ends it on the server.
    #[tokio::test]
    async fn discovery_hands_its_session_to_calls_and_stopping_ends_it() {
        let (url, counts, _) = go_sdk_server().await;
        let manager = BackendManager::new();
        let id = Uuid::new_v4();
        let config = config_for(url);

        let tools = manager.discover_http_tools(id, "go", &config).await;
        assert!(tools.is_ok(), "{tools:?}");
        manager
            .call_http_tool(id, &config, "echo", &echo("hi"))
            .await
            .unwrap();
        assert_eq!(counts.initialize.load(Ordering::SeqCst), 1);

        manager.stop_backend(&id).await;
        eventually("the session to be deleted", || {
            counts.deletes.load(Ordering::SeqCst) == 1
        })
        .await;
    }

    /// An edited backend does not present the session its old configuration
    /// opened.
    #[tokio::test]
    async fn a_changed_configuration_does_not_reuse_the_old_session() {
        let (url, counts, _) = go_sdk_server().await;
        let manager = BackendManager::new();
        let id = Uuid::new_v4();

        let before =
            serde_json::json!({ "url": url, "headers": { "Authorization": "Bearer one" } });
        let after = serde_json::json!({ "url": url, "headers": { "Authorization": "Bearer two" } });
        manager
            .call_http_tool(id, &before, "echo", &echo("a"))
            .await
            .unwrap();
        manager
            .call_http_tool(id, &after, "echo", &echo("b"))
            .await
            .unwrap();
        assert_eq!(counts.initialize.load(Ordering::SeqCst), 2);
    }

    /// A stateless server whose error merely mentions a session. The handshake
    /// finds no session to open, and what the caller hears is the backend's
    /// own error — not a complaint about a missing header — with the call made
    /// once.
    #[tokio::test]
    async fn a_stateless_backends_own_error_is_reported_and_not_replayed() {
        let counts = Arc::new(Counts::default());
        let c = counts.clone();
        let base = wire::serve(move |req, stream| {
            let counts = c.clone();
            async move {
                let id = req.body["id"].clone();
                let body = match req.body["method"].as_str() {
                    Some("initialize") => {
                        counts.initialize.fetch_add(1, Ordering::SeqCst);
                        serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {}})
                    }
                    Some("tools/call") => {
                        counts.tool_calls.fetch_add(1, Ordering::SeqCst);
                        serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": -32000, "message": "Browser session not found"
                        }})
                    }
                    _ => return wire::respond(stream, "202 Accepted", &[], "").await,
                };
                wire::respond(
                    stream,
                    "200 OK",
                    &[("Content-Type", "application/json")],
                    &body.to_string(),
                )
                .await
            }
        })
        .await;

        let manager = BackendManager::new();
        let err = manager
            .call_http_tool(Uuid::new_v4(), &config_for(base), "browse", &echo("x"))
            .await
            .unwrap_err();
        assert!(err.contains("Browser session not found"), "{err}");
        assert_eq!(counts.tool_calls.load(Ordering::SeqCst), 1);
    }

    /// An SSE server built on the Python SDK, byte for byte: CRLF everywhere,
    /// the endpoint as a relative path with the session in the query, every
    /// POST answered `202 Accepted`, replies on the stream, and a keep-alive
    /// comment in between. The stream also carries a server-to-client `ping`
    /// request with the same id as the handshake, which must not be taken for
    /// its reply.
    async fn python_sdk_sse_server(counts: Arc<Counts>) -> String {
        type Streams = std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>;
        let streams: Arc<Streams> = Arc::default();
        wire::serve(move |req, mut stream| {
            let (counts, streams) = (counts.clone(), streams.clone());
            async move {
                use tokio::io::AsyncWriteExt;
                if req.method == "GET" && req.path == "/sse" {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                    streams.lock().unwrap().push(tx);
                    counts.open_streams.fetch_add(1, Ordering::SeqCst);
                    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\ncache-control: no-store\r\nconnection: close\r\n\r\n";
                    let mut ok = stream.write_all(head.as_bytes()).await.is_ok();
                    // Split mid line-ending, the way a network can deliver it.
                    ok &= stream.write_all(b"event: endpoint\r").await.is_ok();
                    let _ = stream.flush().await;
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    ok &= stream
                        .write_all(b"\ndata: /messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a\r\n\r\n")
                        .await
                        .is_ok();
                    while ok {
                        let frame = tokio::select! {
                            frame = rx.recv() => match frame { Some(f) => f, None => break },
                            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => ": ping\r\n\r\n".to_string(),
                        };
                        ok = stream.write_all(frame.as_bytes()).await.is_ok()
                            && stream.flush().await.is_ok();
                    }
                    counts.open_streams.fetch_sub(1, Ordering::SeqCst);
                    return;
                }
                if req.method == "POST" && req.path != "/messages/?session_id=d4acccbc40e0410a81cc70f1f98d475a" {
                    return wire::respond(stream, "404 Not Found", &[], "Could not find session").await;
                }
                let frame = |json: serde_json::Value| format!("event: message\r\ndata: {json}\r\n\r\n");
                let id = req.body["id"].clone();
                let replies: Vec<String> = match req.body["method"].as_str() {
                    Some("initialize") => {
                        counts.initialize.fetch_add(1, Ordering::SeqCst);
                        vec![
                            frame(serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "ping"})),
                            frame(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                                "protocolVersion": "2025-06-18", "capabilities": {"tools": {}}
                            }})),
                        ]
                    }
                    Some("tools/list") => vec![frame(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                        "tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}}]
                    }}))],
                    Some("tools/call") => {
                        counts.tool_calls.fetch_add(1, Ordering::SeqCst);
                        vec![
                            frame(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}})),
                            frame(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                                "content": [{"type": "text", "text": req.body["params"]["arguments"]["text"]}],
                                "isError": false
                            }})),
                        ]
                    }
                    _ => vec![],
                };
                // Accepted first, then the replies on every open stream — the
                // server's order, which is what makes the reply arrive after
                // the POST returns.
                wire::respond(stream, "202 Accepted", &[], "Accepted").await;
                for tx in streams.lock().unwrap().iter() {
                    for reply in &replies {
                        let _ = tx.send(reply.clone());
                    }
                }
            }
        })
        .await
    }

    /// Issue #12, end to end.
    #[tokio::test]
    async fn a_python_sdk_sse_backend_is_discovered_and_called() {
        let counts = Arc::new(Counts::default());
        let base = python_sdk_sse_server(counts.clone()).await;
        let config = config_for(format!("{base}/sse"));

        let tools = BackendManager::discover_sse_tools("androidtv", &config)
            .await
            .unwrap_or_else(|e| panic!("discovery failed: {e}"));
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");

        let result = BackendManager::call_sse_tool(&config, "echo", &echo("hello"))
            .await
            .unwrap_or_else(|e| panic!("call failed: {e}"));
        assert_eq!(result["content"][0]["text"], "hello");
        assert_eq!(counts.tool_calls.load(Ordering::SeqCst), 1);

        // Neither the discovery nor the call leaves its stream open.
        eventually("every SSE stream to close", || {
            counts.open_streams.load(Ordering::SeqCst) == 0
        })
        .await;
    }

    /// A POST the server refuses is reported with its status straight away,
    /// rather than as a thirty-second wait for a reply that is never coming.
    #[tokio::test]
    async fn a_refused_sse_post_fails_fast_with_its_reason() {
        let base = wire::serve(|req, mut stream| async move {
            use tokio::io::AsyncWriteExt;
            if req.method == "GET" {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\nevent: endpoint\r\ndata: /messages/?session_id=gone\r\n\r\n")
                    .await;
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            } else {
                wire::respond(stream, "404 Not Found", &[], "Could not find session").await;
            }
        })
        .await;

        let started = std::time::Instant::now();
        let err = BackendManager::discover_sse_tools("gone", &config_for(format!("{base}/sse")))
            .await
            .unwrap_err();
        assert!(
            err.contains("404") && err.contains("Could not find session"),
            "{err}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// A redirect to another origin is not followed, so the backend's headers
    /// never reach it. The redirect itself comes back as the answer.
    #[tokio::test]
    async fn a_redirect_off_origin_is_not_followed() {
        let reached = Arc::new(AtomicUsize::new(0));
        let r = reached.clone();
        let elsewhere = wire::serve(move |_req, stream| {
            let reached = r.clone();
            async move {
                reached.fetch_add(1, Ordering::SeqCst);
                wire::respond(
                    stream,
                    "200 OK",
                    &[("Content-Type", "application/json")],
                    "{}",
                )
                .await;
            }
        })
        .await;
        let location = format!("{elsewhere}/collect");
        let backend = wire::serve(move |_req, stream| {
            let location = location.clone();
            async move {
                wire::respond(
                    stream,
                    "307 Temporary Redirect",
                    &[("Location", &location)],
                    "",
                )
                .await;
            }
        })
        .await;

        let config = serde_json::json!({ "url": format!("{backend}/mcp"), "headers": { "X-API-Key": "secret" } });
        let err = BackendManager::probe_http_tools("x", &config)
            .await
            .unwrap_err();
        assert!(err.contains("307"), "{err}");
        assert_eq!(
            reached.load(Ordering::SeqCst),
            0,
            "the other origin was contacted"
        );
    }

    #[tokio::test]
    async fn a_redirect_on_the_same_origin_is_followed() {
        let base = wire::serve(|req, stream| async move {
            if req.path == "/mcp" {
                return wire::respond(
                    stream,
                    "307 Temporary Redirect",
                    &[("Location", "/mcp/")],
                    "",
                )
                .await;
            }
            let body = match req.body["method"].as_str() {
                Some("initialize") => {
                    serde_json::json!({"jsonrpc": "2.0", "id": req.body["id"], "result": {}})
                }
                Some("tools/list") => {
                    serde_json::json!({"jsonrpc": "2.0", "id": req.body["id"], "result": {
                        "tools": [{"name": "echo"}]
                    }})
                }
                _ => return wire::respond(stream, "202 Accepted", &[], "").await,
            };
            wire::respond(
                stream,
                "200 OK",
                &[("Content-Type", "application/json")],
                &body.to_string(),
            )
            .await
        })
        .await;
        let count = BackendManager::probe_http_tools("x", &config_for(format!("{base}/mcp")))
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Pointed at a streamable-http server by mistake, the SSE transport says
    /// what it found rather than only that it waited.
    #[tokio::test]
    async fn an_sse_url_that_is_not_an_event_stream_says_so() {
        let base = wire::serve(|_req, stream| async move {
            wire::respond(
                stream,
                "405 Method Not Allowed",
                &[("allow", "POST, DELETE")],
                "Method Not Allowed",
            )
            .await;
        })
        .await;
        let err = BackendManager::discover_sse_tools("wrong", &config_for(format!("{base}/mcp")))
            .await
            .unwrap_err();
        assert!(
            err.contains("405") && err.contains("streamable-http"),
            "{err}"
        );
    }
}

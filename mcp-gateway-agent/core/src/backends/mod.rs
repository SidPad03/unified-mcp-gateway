//! The local MCP servers this machine puts behind the gateway.
//!
//! Three defects from the old agent shaped this module:
//!
//! * Backends started **sequentially**, each awaiting `initialize` and
//!   `tools/list`, so ten backends could block startup for minutes (#1). They
//!   now start concurrently, one supervisor task each, and the UI paints
//!   immediately with rows that move `starting → ready / failed`.
//! * A single failing backend returned `Err` from `start_all` and took the whole
//!   agent down (#2). A failure is now a row with an error on it, and nothing
//!   more.
//! * The manager was immutable after startup, so adding or removing a backend
//!   meant restarting the agent (#5). State lives behind `RwLock`s and
//!   `add`/`remove`/`update`/`restart`/`set_enabled` all work while connected,
//!   with a debounced re-`register` to the gateway.

pub mod http;
pub mod stdio;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

use crate::config::LocalBackendConfig;
use crate::logbuf::LogBuffer;
use crate::protocol::{SubBackendInfo, ToolInfo};

// ── PATH ────────────────────────────────────────────────────────────────

static LOGIN_PATH: OnceLock<String> = OnceLock::new();

fn fallback_path() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let mut parts: Vec<String> = vec![
        "/opt/homebrew/bin".into(),
        "/opt/homebrew/sbin".into(),
        "/usr/local/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
    ];
    for suffix in [".local/bin", ".cargo/bin", ".bun/bin", ".deno/bin"] {
        parts.push(home.join(suffix).display().to_string());
    }
    parts.join(":")
}

/// Work out the `PATH` a backend should be started with, once, at launch.
///
/// This matters more than it looks. An app launched from Finder inherits
/// launchd's `PATH` — `/usr/bin:/bin:/usr/sbin:/sbin` — not the user's. Every
/// MCP server people actually run (`uvx`, `npx`, `bun`, anything from Homebrew)
/// lives somewhere else, so without this the app would spawn nothing and report
/// "No such file or directory" for backends that work perfectly in a terminal.
/// The old agent never hit this because it only ever ran from a shell.
///
/// Asking the user's login shell is the reliable way to get their real `PATH`.
/// It is also the way to get stuck behind an interactive `.zshrc`, hence the
/// timeout and the fallback.
pub async fn init_login_path() {
    if LOGIN_PATH.get().is_some() {
        return;
    }
    let resolved = resolve_login_path().await;
    let _ = LOGIN_PATH.set(resolved);
}

async fn resolve_login_path() -> String {
    let fallback = fallback_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::process::Command::new(&shell)
            .args(["-ilc", r#"printf %s "$PATH""#])
            .env("TERM", "dumb")
            .output(),
    )
    .await;

    let shell_path = match output {
        Ok(Ok(out)) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => {
            tracing::warn!(
                shell = %shell,
                "Could not read PATH from the login shell; using built-in defaults"
            );
            String::new()
        }
    };

    // Union, shell's order first, so a user who put a version manager ahead of
    // Homebrew keeps that ordering.
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::new();
    for part in shell_path.split(':').chain(fallback.split(':')) {
        let part = part.trim();
        if !part.is_empty() && seen.insert(part.to_string()) {
            parts.push(part.to_string());
        }
    }
    parts.join(":")
}

pub fn login_path() -> String {
    LOGIN_PATH.get().cloned().unwrap_or_else(fallback_path)
}

// ── MCP helpers ─────────────────────────────────────────────────────────

pub fn initialize_params() -> Value {
    serde_json::json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": { "name": "mcp-gateway-agent", "version": crate::VERSION }
    })
}

/// Turn a `tools/list` result into namespaced [`ToolInfo`]s.
pub fn parse_tools(result: &Value, backend: &str) -> Vec<ToolInfo> {
    let Some(raw) = result.get("tools").and_then(Value::as_array) else {
        return Vec::new();
    };
    raw.iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            if name.is_empty() {
                return None;
            }
            Some(ToolInfo {
                name: format!("{backend}__{name}"),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                input_schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
            })
        })
        .collect()
}

// ── Runtime state ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStatus {
    /// Turned off in the config: not spawned, not registered.
    Disabled,
    Starting,
    Ready,
    /// Could not start (bad command, refused connection, failed handshake).
    Failed,
    /// Started, then the process exited on its own.
    Crashed,
    /// Deliberately stopped, e.g. mid-restart.
    Stopped,
}

impl BackendStatus {
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Debug, Clone)]
pub struct BackendRuntime {
    pub status: BackendStatus,
    pub error: Option<String>,
    pub pid: Option<u32>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub restarts: u32,
    pub tools: Vec<ToolInfo>,
}

impl Default for BackendRuntime {
    fn default() -> Self {
        Self {
            status: BackendStatus::Starting,
            error: None,
            pid: None,
            started_at: None,
            restarts: 0,
            tools: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub enum Client {
    Stdio(stdio::StdioClient),
    Http(http::HttpClient),
}

/// Instructions for a backend's supervisor task.
pub enum BackendCmd {
    Restart,
    Stop,
    Reload(Box<LocalBackendConfig>),
    SetEnabled(bool),
}

pub struct Backend {
    pub name: String,
    config: RwLock<LocalBackendConfig>,
    runtime: RwLock<BackendRuntime>,
    client: RwLock<Option<Client>>,
    ctl: mpsc::UnboundedSender<BackendCmd>,
}

impl Backend {
    fn new(config: LocalBackendConfig) -> (Arc<Self>, mpsc::UnboundedReceiver<BackendCmd>) {
        let (ctl, rx) = mpsc::unbounded_channel();
        let backend = Arc::new(Self {
            name: config.name.clone(),
            config: RwLock::new(config),
            runtime: RwLock::new(BackendRuntime::default()),
            client: RwLock::new(None),
            ctl,
        });
        (backend, rx)
    }

    pub async fn config(&self) -> LocalBackendConfig {
        self.config.read().await.clone()
    }

    pub async fn runtime(&self) -> BackendRuntime {
        self.runtime.read().await.clone()
    }

    pub async fn client(&self) -> Option<Client> {
        self.client.read().await.clone()
    }

    pub(crate) async fn set_client(&self, client: Option<Client>) {
        *self.client.write().await = client;
    }

    pub(crate) async fn set_config(&self, config: LocalBackendConfig) {
        *self.config.write().await = config;
    }

    pub(crate) async fn update_runtime(&self, f: impl FnOnce(&mut BackendRuntime)) {
        f(&mut *self.runtime.write().await);
    }

    pub(crate) fn send(&self, cmd: BackendCmd) {
        // The receiver only goes away when the supervisor has exited, which
        // happens after Stop. Nothing to do about a command sent after that.
        let _ = self.ctl.send(cmd);
    }
}

// ── Routing ─────────────────────────────────────────────────────────────

/// Namespaced tool name → backend name.
///
/// Kept beside the manager rather than inside it so supervisors can update
/// routes without holding a reference back to the manager that owns them.
#[derive(Default)]
pub struct RouteTable {
    map: RwLock<HashMap<String, String>>,
}

impl RouteTable {
    pub async fn replace(&self, backend: &str, tools: &[ToolInfo]) {
        let mut map = self.map.write().await;
        map.retain(|_, owner| owner != backend);
        for tool in tools {
            map.insert(tool.name.clone(), backend.to_string());
        }
    }

    pub async fn remove_backend(&self, backend: &str) {
        self.map.write().await.retain(|_, owner| owner != backend);
    }

    pub async fn lookup(&self, tool: &str) -> Option<String> {
        self.map.read().await.get(tool).cloned()
    }
}

/// The handful of shared things every supervisor needs.
#[derive(Clone)]
pub struct CoreHooks {
    pub logs: Arc<LogBuffer>,
    pub routes: Arc<RouteTable>,
    /// Capacity-1 channel: `try_send` coalesces a burst of changes into one
    /// wake-up, and the debouncer downstream turns that into a single
    /// re-`register`.
    pub reregister: mpsc::Sender<()>,
    /// Bumped on every observable change, so the UI layer can skip emitting an
    /// identical snapshot ten times a second.
    pub generation: Arc<AtomicU64>,
}

impl CoreHooks {
    pub fn touch(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn request_reregister(&self) {
        let _ = self.reregister.try_send(());
    }
}

// ── Views (what crosses the FFI boundary) ───────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ToolSummary {
    pub name: String,
    pub description: String,
}

/// One environment variable or header, as the app is allowed to see it.
#[derive(Debug, Clone, Serialize)]
pub struct SettingView {
    pub key: String,
    /// `None` when the variable is masked. The value is still in `config.toml`
    /// — it is simply not something the UI gets to render until the user clears
    /// the mask in the editor.
    pub value: Option<String>,
    pub masked: bool,
}

fn settings_view(values: &HashMap<String, String>, masked: &[String]) -> Vec<SettingView> {
    let mut keys: Vec<&String> = values.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|key| {
            let is_masked = masked.iter().any(|m| m == key);
            SettingView {
                key: key.clone(),
                value: if is_masked {
                    None
                } else {
                    values.get(key).cloned()
                },
                masked: is_masked,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendView {
    pub name: String,
    pub transport: String,
    pub enabled: bool,
    pub status: BackendStatus,
    pub error: Option<String>,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub uptime_secs: Option<i64>,
    pub restarts: u32,
    pub tool_count: usize,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    /// Every variable by name; the value only when it is not masked.
    pub env: Vec<SettingView>,
    pub headers: Vec<SettingView>,
    pub tools: Vec<ToolSummary>,
}

// ── Manager ─────────────────────────────────────────────────────────────

pub struct BackendManager {
    entries: RwLock<Vec<Arc<Backend>>>,
    hooks: CoreHooks,
}

impl BackendManager {
    pub fn new(hooks: CoreHooks) -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
            hooks,
        })
    }

    pub fn hooks(&self) -> &CoreHooks {
        &self.hooks
    }

    /// Bring up every configured backend **concurrently** and return at once.
    ///
    /// Nothing here awaits a handshake. The caller connects to the gateway
    /// straight away and registers whatever is ready; each backend that comes up
    /// later triggers a debounced re-register on its own.
    pub async fn start_all(&self, configs: &[LocalBackendConfig]) {
        for config in configs {
            self.spawn_entry(config.clone()).await;
        }
    }

    async fn spawn_entry(&self, config: LocalBackendConfig) {
        let (backend, rx) = Backend::new(config);
        self.entries.write().await.push(backend.clone());
        crate::supervisor::spawn(backend, rx, self.hooks.clone());
        self.hooks.touch();
    }

    async fn find(&self, name: &str) -> Option<Arc<Backend>> {
        self.entries
            .read()
            .await
            .iter()
            .find(|b| b.name == name)
            .cloned()
    }

    pub async fn add(&self, config: LocalBackendConfig) -> Result<(), String> {
        config.validate()?;
        if self.find(&config.name).await.is_some() {
            return Err(format!("A backend named '{}' already exists", config.name));
        }
        self.spawn_entry(config).await;
        Ok(())
    }

    pub async fn remove(&self, name: &str) -> Result<(), String> {
        let backend = self
            .find(name)
            .await
            .ok_or_else(|| format!("No backend named '{name}'"))?;
        backend.send(BackendCmd::Stop);
        self.entries.write().await.retain(|b| b.name != name);
        self.hooks.routes.remove_backend(name).await;
        self.hooks.touch();
        self.hooks.request_reregister();
        Ok(())
    }

    /// Replace a backend's configuration and restart it under the new one.
    ///
    /// Renaming is handled as remove-then-add, because the name is the routing
    /// namespace and half-renamed routes are not worth the cleverness.
    pub async fn update(&self, name: &str, config: LocalBackendConfig) -> Result<(), String> {
        config.validate()?;
        if config.name != name {
            if self.find(&config.name).await.is_some() {
                return Err(format!("A backend named '{}' already exists", config.name));
            }
            self.remove(name).await?;
            return self.add(config).await;
        }
        let backend = self
            .find(name)
            .await
            .ok_or_else(|| format!("No backend named '{name}'"))?;
        backend.set_config(config.clone()).await;
        backend.send(BackendCmd::Reload(Box::new(config)));
        self.hooks.touch();
        Ok(())
    }

    pub async fn restart(&self, name: &str) -> Result<(), String> {
        let backend = self
            .find(name)
            .await
            .ok_or_else(|| format!("No backend named '{name}'"))?;
        backend.send(BackendCmd::Restart);
        Ok(())
    }

    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        let backend = self
            .find(name)
            .await
            .ok_or_else(|| format!("No backend named '{name}'"))?;
        let mut config = backend.config().await;
        config.enabled = enabled;
        backend.set_config(config).await;
        backend.send(BackendCmd::SetEnabled(enabled));
        self.hooks.touch();
        Ok(())
    }

    pub async fn route_of(&self, tool: &str) -> Option<String> {
        self.hooks.routes.lookup(tool).await
    }

    /// Route a namespaced tool call to its backend.
    pub async fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        let backend_name = self
            .hooks
            .routes
            .lookup(tool)
            .await
            .ok_or_else(|| format!("No local backend provides '{tool}'"))?;
        let backend = self
            .find(&backend_name)
            .await
            .ok_or_else(|| format!("Backend '{backend_name}' is gone"))?;

        let bare = tool
            .strip_prefix(&format!("{backend_name}__"))
            .unwrap_or(tool)
            .to_string();

        // Clone the client out before awaiting: both variants are cheap Arc
        // clones, and holding the RwLock across a 120-second tool call would
        // block every other operation on this backend.
        let client = backend
            .client()
            .await
            .ok_or_else(|| format!("Backend '{backend_name}' is not running"))?;

        match client {
            Client::Stdio(c) => c.call_tool(&bare, arguments).await,
            Client::Http(c) => c.call_tool(&bare, arguments).await,
        }
    }

    /// Tools from backends that actually came up.
    ///
    /// Defect #10: the old agent registered every configured backend's tools as
    /// though all were healthy, so the gateway advertised tools that could not
    /// possibly answer.
    pub async fn ready_tools(&self) -> Vec<ToolInfo> {
        let entries = self.entries.read().await.clone();
        let mut tools = Vec::new();
        for backend in entries {
            let runtime = backend.runtime().await;
            if runtime.status.is_ready() {
                tools.extend(runtime.tools.iter().cloned());
            }
        }
        tools
    }

    pub async fn ready_sub_backends(&self) -> Vec<SubBackendInfo> {
        let entries = self.entries.read().await.clone();
        let mut infos = Vec::new();
        for backend in entries {
            let runtime = backend.runtime().await;
            if !runtime.status.is_ready() {
                continue;
            }
            let config = backend.config().await;
            let mut env_keys: Vec<String> = config.env.keys().cloned().collect();
            env_keys.sort();
            let mut env_masked = config.masked_env.clone();
            env_masked.sort();
            infos.push(SubBackendInfo {
                name: config.name,
                transport: config.transport,
                command: config.command,
                args: config.args,
                url: config.url,
                env_keys,
                env_masked,
                tool_count: runtime.tools.len(),
            });
        }
        infos
    }

    pub async fn snapshot(&self) -> Vec<BackendView> {
        let entries = self.entries.read().await.clone();
        let now = chrono::Utc::now();
        let mut views = Vec::with_capacity(entries.len());
        for backend in entries {
            let config = backend.config().await;
            let runtime = backend.runtime().await;
            let env = settings_view(&config.env, &config.masked_env);
            let headers = settings_view(&config.headers, &config.masked_headers);
            views.push(BackendView {
                name: config.name,
                transport: config.transport,
                enabled: config.enabled,
                status: runtime.status,
                error: runtime.error.clone(),
                pid: runtime.pid,
                started_at: runtime
                    .started_at
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                uptime_secs: runtime.started_at.map(|t| (now - t).num_seconds().max(0)),
                restarts: runtime.restarts,
                tool_count: runtime.tools.len(),
                command: config.command,
                args: config.args,
                url: config.url,
                env,
                headers,
                tools: runtime
                    .tools
                    .iter()
                    .map(|t| ToolSummary {
                        name: t.name.clone(),
                        description: t.description.clone(),
                    })
                    .collect(),
            });
        }
        views
    }

    /// `(ready, total)` — the Overview hero's "backends up".
    pub async fn counts(&self) -> (usize, usize) {
        let entries = self.entries.read().await.clone();
        let mut ready = 0;
        let mut total = 0;
        for backend in entries {
            let config = backend.config().await;
            if !config.enabled {
                continue;
            }
            total += 1;
            if backend.runtime().await.status.is_ready() {
                ready += 1;
            }
        }
        (ready, total)
    }

    pub async fn shutdown(&self) {
        let entries = std::mem::take(&mut *self.entries.write().await);
        for backend in entries {
            backend.send(BackendCmd::Stop);
        }
    }
}

// ── Test connection ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub tool_count: usize,
    pub tools: Vec<String>,
    pub took_ms: u64,
}

/// Try a backend configuration without adopting it.
///
/// This is what the Add / Edit sheet's "Test connection" runs, so a bad command
/// is caught before it is written to the config rather than after.
pub async fn test_connection(config: &LocalBackendConfig) -> Result<TestResult, String> {
    config.validate()?;
    let started = std::time::Instant::now();

    let tools = if config.is_stdio() {
        let mut spawned = stdio::spawn(config)?;
        // Drain stderr so a chatty server cannot fill the pipe buffer and wedge
        // itself while we are still talking to it.
        let mut stderr = spawned.stderr;
        let drain = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut sink = Vec::new();
            let _ = stderr.read_to_end(&mut sink).await;
        });

        let result = async {
            spawned
                .session
                .request("initialize", initialize_params(), stdio::INITIALIZE_TIMEOUT)
                .await?;
            let _ = spawned
                .session
                .notify("notifications/initialized", serde_json::json!({}))
                .await;
            spawned
                .session
                .request(
                    "tools/list",
                    serde_json::json!({}),
                    stdio::LIST_TOOLS_TIMEOUT,
                )
                .await
        }
        .await;

        let _ = spawned.child.kill().await;
        drain.abort();
        parse_tools(&result?, &config.name)
    } else {
        let client = http::HttpClient::new(config)?;
        client.initialize().await?;
        parse_tools(&client.list_tools().await?, &config.name)
    };

    Ok(TestResult {
        tool_count: tools.len(),
        tools: tools.into_iter().map(|t| t.name).collect(),
        took_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hooks() -> (CoreHooks, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel(1);
        (
            CoreHooks {
                logs: Arc::new(LogBuffer::new()),
                routes: Arc::new(RouteTable::default()),
                reregister: tx,
                generation: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    #[test]
    fn tools_are_namespaced_by_backend() {
        let raw = serde_json::json!({
            "tools": [
                {"name": "get_scene_info", "description": "d", "inputSchema": {"type": "object"}},
                {"name": "", "description": "skipped"},
                {"description": "no name at all"}
            ]
        });
        let tools = parse_tools(&raw, "blender");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "blender__get_scene_info");
    }

    #[test]
    fn a_tools_list_without_tools_is_empty_not_an_error() {
        assert!(parse_tools(&serde_json::json!({}), "x").is_empty());
    }

    #[test]
    fn a_tool_without_a_schema_gets_an_empty_object_schema() {
        let raw = serde_json::json!({"tools": [{"name": "t"}]});
        let tools = parse_tools(&raw, "b");
        assert_eq!(tools[0].input_schema["type"], "object");
        assert_eq!(tools[0].description, "");
    }

    #[tokio::test]
    async fn routes_are_replaced_wholesale_per_backend() {
        let routes = RouteTable::default();
        let make = |names: &[&str]| -> Vec<ToolInfo> {
            names
                .iter()
                .map(|n| ToolInfo {
                    name: (*n).to_string(),
                    description: String::new(),
                    input_schema: Value::Null,
                })
                .collect()
        };

        routes.replace("a", &make(&["a__one", "a__two"])).await;
        routes.replace("b", &make(&["b__one"])).await;
        assert_eq!(routes.lookup("a__two").await.as_deref(), Some("a"));

        // A restart that discovers fewer tools must drop the stale ones.
        routes.replace("a", &make(&["a__one"])).await;
        assert_eq!(routes.lookup("a__one").await.as_deref(), Some("a"));
        assert_eq!(routes.lookup("a__two").await, None);
        assert_eq!(
            routes.lookup("b__one").await.as_deref(),
            Some("b"),
            "other backends untouched"
        );

        routes.remove_backend("a").await;
        assert_eq!(routes.lookup("a__one").await, None);
    }

    #[tokio::test]
    async fn adding_a_duplicate_name_is_rejected() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        let config = LocalBackendConfig {
            name: "dup".into(),
            transport: "stdio".into(),
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), "sleep 30".into()],
            ..Default::default()
        };
        manager.add(config.clone()).await.unwrap();
        let err = manager.add(config).await.unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn an_invalid_backend_is_rejected_before_it_is_ever_spawned() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        let err = manager
            .add(LocalBackendConfig {
                name: "broken".into(),
                transport: "stdio".into(),
                command: None,
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(err.contains("command"), "{err}");
        assert!(manager.snapshot().await.is_empty());
    }

    #[tokio::test]
    async fn a_masked_value_never_reaches_the_snapshot() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        manager
            .add(LocalBackendConfig {
                name: "secretive".into(),
                transport: "stdio".into(),
                command: Some("/bin/sh".into()),
                args: vec!["-c".into(), "sleep 30".into()],
                env: HashMap::from([
                    ("TOKEN".into(), "hunter2".into()),
                    ("MODE".into(), "debug".into()),
                ]),
                masked_env: vec!["TOKEN".into()],
                ..Default::default()
            })
            .await
            .unwrap();

        let views = manager.snapshot().await;
        let json = serde_json::to_string(&views[0]).unwrap();
        assert!(!json.contains("hunter2"), "masked value leaked: {json}");
        assert!(
            json.contains("TOKEN"),
            "the key is still shown, only the value is not: {json}"
        );
        assert!(
            json.contains("debug"),
            "an unmasked value is plain text: {json}"
        );
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn calling_an_unrouted_tool_names_the_tool() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        let err = manager
            .call_tool("ghost__vanish", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(err.contains("ghost__vanish"), "{err}");
    }

    #[tokio::test]
    async fn test_connection_reports_a_command_that_does_not_exist() {
        let err = test_connection(&LocalBackendConfig {
            name: "probe".into(),
            transport: "stdio".into(),
            command: Some("definitely-not-a-real-binary-xyz".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
        assert!(err.contains("definitely-not-a-real-binary-xyz"), "{err}");
    }

    #[test]
    fn the_fallback_path_covers_where_mcp_servers_actually_live() {
        let path = fallback_path();
        for expected in ["/opt/homebrew/bin", "/usr/local/bin", ".local/bin"] {
            assert!(path.contains(expected), "{expected} missing from {path}");
        }
    }
}

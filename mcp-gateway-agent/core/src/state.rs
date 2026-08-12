//! Everything the app knows, in one place.
//!
//! [`AgentState`] owns the config, the credential, the backend manager and the
//! two ring buffers, and it is the only thing the FFI layer holds. Mutations
//! that change what the gateway should know about — a backend added, removed,
//! enabled, or coming up after a crash — funnel into a single debounced
//! re-`register`, so a burst of five changes costs one frame on the wire rather
//! than five.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, Notify, RwLock};

use crate::backends::{BackendManager, BackendView, CoreHooks, RouteTable};
use crate::config::{self, Config, ConfigView, LocalBackendConfig};
use crate::logbuf::{CallBuffer, LogBuffer, LogLevel};
use crate::protocol::AgentMessage;

/// How long the agent waits for the changes to stop before telling the gateway
/// about them.
const REREGISTER_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    /// Not configured yet — the first-run wizard is showing.
    Idle,
    Connecting,
    Connected,
    Reconnecting,
    /// Connected once, then failed in a way that is not being retried.
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub state: ConnState,
    pub gateway_url: String,
    pub agent_id: String,
    /// The id the gateway filed us under; also the `backend` filter for the
    /// Audit and Usage pages.
    pub backend_id: Option<String>,
    pub connected_since: Option<String>,
    pub attempt: u32,
    pub last_error: Option<String>,
    pub retry_in_ms: Option<u64>,
    pub registered_tools: usize,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            state: ConnState::Idle,
            gateway_url: String::new(),
            agent_id: String::new(),
            backend_id: None,
            connected_since: None,
            attempt: 0,
            last_error: None,
            retry_in_ms: None,
            registered_tools: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub tools_registered: usize,
    pub backends_ready: usize,
    pub backends_total: usize,
    pub calls_total: u64,
    pub calls_errors: u64,
    pub log_lines_dropped: u64,
}

/// The whole of the UI's model, in one payload.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub connection: ConnectionStatus,
    pub backends: Vec<BackendView>,
    pub config: ConfigView,
    pub stats: Stats,
    pub generation: u64,
    pub version: String,
    pub uptime_secs: i64,
}

pub struct AgentState {
    pub config_path: PathBuf,
    config: RwLock<Config>,
    /// Held in memory only; the on-disk copy lives in the Keychain and the
    /// value never appears in a snapshot, a log line, or an error message.
    api_key: RwLock<String>,
    pub backends: Arc<BackendManager>,
    pub logs: Arc<LogBuffer>,
    pub calls: Arc<CallBuffer>,
    connection: RwLock<ConnectionStatus>,
    /// Set while a tunnel is connected; the sink for outgoing frames.
    writer: RwLock<Option<mpsc::Sender<String>>>,
    /// Kicks the tunnel: reconnect now, or pick up a config change.
    pub(crate) reconnect: Arc<Notify>,
    hooks: CoreHooks,
    started_at: chrono::DateTime<chrono::Utc>,
}

impl AgentState {
    /// Build the state from a config file. Nothing is started yet.
    pub fn new(config_path: PathBuf, config: Config) -> Arc<Self> {
        let logs = Arc::new(LogBuffer::new());
        let (reregister_tx, reregister_rx) = mpsc::channel(1);
        let hooks = CoreHooks {
            logs: logs.clone(),
            routes: Arc::new(RouteTable::default()),
            reregister: reregister_tx,
            generation: Arc::new(AtomicU64::new(1)),
        };

        let mut connection = ConnectionStatus {
            gateway_url: config.agent.gateway_url.clone(),
            agent_id: config.agent.agent_id.clone(),
            ..Default::default()
        };
        connection.state = ConnState::Idle;

        let state = Arc::new(Self {
            config_path,
            config: RwLock::new(config),
            api_key: RwLock::new(String::new()),
            backends: BackendManager::new(hooks.clone()),
            logs,
            calls: Arc::new(CallBuffer::new()),
            connection: RwLock::new(connection),
            writer: RwLock::new(None),
            reconnect: Arc::new(Notify::new()),
            hooks,
            started_at: chrono::Utc::now(),
        });

        tokio::spawn(debounce_reregister(state.clone(), reregister_rx));
        state
    }

    /// Start the backends and the tunnel.
    ///
    /// Returns immediately. Backends come up concurrently in the background and
    /// the tunnel connects in parallel with them, so the window paints at once
    /// rather than after the slowest `tools/list`.
    pub async fn start(self: &Arc<Self>) {
        crate::backends::init_login_path().await;
        let backends = self.config.read().await.backends.clone();
        self.backends.start_all(&backends).await;
        tokio::spawn(crate::tunnel::run(self.clone()));
    }

    // ── Config ──────────────────────────────────────────────────────────

    pub async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    pub async fn config_view(&self) -> ConfigView {
        let config = self.config.read().await;
        let has_key = !self.api_key.read().await.is_empty();
        ConfigView::new(&config, &self.config_path, has_key)
    }

    pub async fn api_key(&self) -> String {
        self.api_key.read().await.clone()
    }

    pub async fn set_api_key(&self, key: String) {
        let changed = *self.api_key.read().await != key;
        *self.api_key.write().await = key;
        if changed {
            self.hooks.touch();
            self.reconnect.notify_one();
        }
    }

    /// Take the legacy plaintext key out of the config, if there is one.
    ///
    /// The caller (the app) puts it in the Keychain and then calls
    /// [`Self::persist`], which writes a config file without it.
    pub async fn take_legacy_api_key(&self) -> Option<String> {
        self.config
            .write()
            .await
            .agent
            .api_key
            .take()
            .filter(|k| !k.trim().is_empty())
    }

    pub async fn persist(&self) -> anyhow::Result<()> {
        let config = self.config.read().await;
        config::save(&self.config_path, &config)
    }

    /// Apply the gateway settings from the wizard or the Settings page.
    pub async fn apply_settings(
        &self,
        agent_id: String,
        gateway_url: String,
        dashboard_url: Option<String>,
        tls_skip_verify: bool,
    ) -> anyhow::Result<()> {
        {
            let mut config = self.config.write().await;
            config.agent.agent_id = agent_id.trim().to_string();
            config.agent.gateway_url = gateway_url.trim().to_string();
            config.agent.dashboard_url = dashboard_url
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty());
            config.agent.tls_skip_verify = tls_skip_verify;
        }
        self.persist().await?;
        {
            let config = self.config.read().await;
            let mut connection = self.connection.write().await;
            connection.agent_id = config.agent.agent_id.clone();
            connection.gateway_url = config.agent.gateway_url.clone();
        }
        self.hooks.touch();
        // Reconnect under the new settings rather than waiting out a backoff.
        self.reconnect.notify_one();
        Ok(())
    }

    // ── Backends (runtime and config, kept in step) ─────────────────────

    /// Put real values back where the editor sent [`crate::config::MASKED`].
    ///
    /// Every path that takes a backend configuration from the app goes through
    /// here first — add, update, and test alike — because the sheet is never
    /// given a masked value and would otherwise hand back a placeholder for one.
    /// `name` is the backend's *current* name, so a rename still finds the
    /// configuration the placeholders belong to.
    pub async fn resolve_masked(
        &self,
        name: &str,
        mut backend: LocalBackendConfig,
    ) -> LocalBackendConfig {
        let previous = self
            .config
            .read()
            .await
            .backend(name)
            .cloned()
            .unwrap_or_default();
        backend.restore_masked_from(&previous);
        backend
    }

    pub async fn add_backend(&self, backend: LocalBackendConfig) -> Result<(), String> {
        let name = backend.name.clone();
        let backend = self.resolve_masked(&name, backend).await;
        self.backends.add(backend.clone()).await?;
        self.config.write().await.backends.push(backend);
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn update_backend(
        &self,
        name: &str,
        backend: LocalBackendConfig,
    ) -> Result<(), String> {
        let backend = self.resolve_masked(name, backend).await;
        self.backends.update(name, backend.clone()).await?;
        {
            let mut config = self.config.write().await;
            match config.backends.iter_mut().find(|b| b.name == name) {
                Some(existing) => *existing = backend,
                None => config.backends.push(backend),
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn remove_backend(&self, name: &str) -> Result<(), String> {
        self.backends.remove(name).await?;
        self.config
            .write()
            .await
            .backends
            .retain(|b| b.name != name);
        self.persist().await.map_err(|e| e.to_string())
    }

    pub async fn set_backend_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        self.backends.set_enabled(name, enabled).await?;
        {
            let mut config = self.config.write().await;
            if let Some(existing) = config.backends.iter_mut().find(|b| b.name == name) {
                existing.enabled = enabled;
            }
        }
        self.persist().await.map_err(|e| e.to_string())
    }

    // ── Connection ──────────────────────────────────────────────────────

    pub async fn connection(&self) -> ConnectionStatus {
        self.connection.read().await.clone()
    }

    pub(crate) async fn update_connection(&self, f: impl FnOnce(&mut ConnectionStatus)) {
        f(&mut *self.connection.write().await);
        self.hooks.touch();
    }

    pub(crate) async fn set_writer(&self, writer: Option<mpsc::Sender<String>>) {
        *self.writer.write().await = writer;
    }

    pub(crate) async fn writer(&self) -> Option<mpsc::Sender<String>> {
        self.writer.read().await.clone()
    }

    /// Send the current tool set to the gateway. No-op when not connected — the
    /// next successful connection registers from scratch anyway.
    pub async fn send_register(&self) -> bool {
        let Some(writer) = self.writer().await else {
            return false;
        };
        let agent_id = self.config.read().await.agent.agent_id.clone();
        let tools = self.backends.ready_tools().await;
        let backends = self.backends.ready_sub_backends().await;
        let count = tools.len();

        let frame = AgentMessage::Register {
            agent_id,
            tools,
            backends,
        }
        .to_frame();

        if writer.send(frame).await.is_err() {
            return false;
        }
        self.update_connection(|c| c.registered_tools = count).await;
        tracing::info!(tool_count = count, "Registered tools with the gateway");
        true
    }

    /// Drop the current connection and start a new one now.
    pub fn request_reconnect(&self) {
        self.reconnect.notify_one();
    }

    pub fn request_reregister(&self) {
        self.hooks.request_reregister();
    }

    // ── Snapshot ────────────────────────────────────────────────────────

    pub fn generation(&self) -> u64 {
        self.hooks.generation.load(Ordering::Relaxed)
    }

    pub async fn snapshot(&self) -> Snapshot {
        let (ready, total) = self.backends.counts().await;
        let (calls_total, calls_errors) = self.calls.totals();
        let connection = self.connection().await;
        Snapshot {
            stats: Stats {
                tools_registered: connection.registered_tools,
                backends_ready: ready,
                backends_total: total,
                calls_total,
                calls_errors,
                log_lines_dropped: self.logs.dropped(),
            },
            connection,
            backends: self.backends.snapshot().await,
            config: self.config_view().await,
            generation: self.generation(),
            version: crate::VERSION.to_string(),
            uptime_secs: (chrono::Utc::now() - self.started_at).num_seconds().max(0),
        }
    }

    pub async fn shutdown(&self) {
        self.logs
            .push(LogLevel::Info, "agent", "Stopping local backends");
        self.backends.shutdown().await;
        self.set_writer(None).await;
    }
}

/// Collapse a burst of backend changes into one re-`register`.
///
/// A restart that discovers seventeen tools fires one change; ten backends
/// coming up at once during launch fire ten. Sending a register frame for each
/// would make the gateway rewrite its tool registry ten times for one settled
/// state. The channel has capacity 1 and the producers use `try_send`, so the
/// burst is already collapsed by the time it gets here; this adds the quiet
/// period on top.
async fn debounce_reregister(state: Arc<AgentState>, mut rx: mpsc::Receiver<()>) {
    while rx.recv().await.is_some() {
        loop {
            match tokio::time::timeout(REREGISTER_DEBOUNCE, rx.recv()).await {
                // Another change arrived inside the window: wait again.
                Ok(Some(())) => continue,
                // Quiet, or the state is gone.
                Ok(None) => return,
                Err(_) => break,
            }
        }
        state.send_register().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn state() -> Arc<AgentState> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // Leak the temp dir: the state outlives this helper and still needs the
        // directory to exist for `persist`.
        std::mem::forget(dir);
        AgentState::new(path, Config::default())
    }

    #[tokio::test]
    async fn a_fresh_state_is_idle_and_unconfigured() {
        let state = state().await;
        let snapshot = state.snapshot().await;
        assert_eq!(snapshot.connection.state, ConnState::Idle);
        assert!(!snapshot.config.configured);
        assert!(!snapshot.config.has_api_key);
    }

    #[tokio::test]
    async fn the_snapshot_never_carries_the_api_key() {
        let state = state().await;
        state.set_api_key("mcpgw_supersecretvalue".into()).await;
        let json = serde_json::to_string(&state.snapshot().await).unwrap();
        assert!(!json.contains("mcpgw_supersecretvalue"), "{json}");
        assert!(json.contains("\"has_api_key\":true"));
    }

    #[tokio::test]
    async fn applying_settings_writes_them_to_disk() {
        let state = state().await;
        state
            .apply_settings(
                "  my-mac  ".into(),
                " wss://gw.example.com/agent/ws ".into(),
                Some("  ".into()),
                true,
            )
            .await
            .unwrap();

        let reloaded = config::load(&state.config_path).unwrap();
        assert_eq!(reloaded.agent.agent_id, "my-mac", "values are trimmed");
        assert_eq!(reloaded.agent.gateway_url, "wss://gw.example.com/agent/ws");
        assert_eq!(reloaded.agent.dashboard_url, None, "blank means unset");
        assert!(reloaded.agent.tls_skip_verify);

        let view = state.config_view().await;
        assert_eq!(view.agent_id, "my-mac");
    }

    #[tokio::test]
    async fn taking_the_legacy_key_leaves_the_config_clean() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.agent.agent_id = "mac".into();
        config.agent.gateway_url = "wss://gw/agent/ws".into();
        config.agent.api_key = Some("mcpgw_legacyvaluehere".into());
        let state = AgentState::new(path.clone(), config);

        let key = state.take_legacy_api_key().await.unwrap();
        assert_eq!(key, "mcpgw_legacyvaluehere");
        state.set_api_key(key).await;
        state.persist().await.unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains("mcpgw_"), "{on_disk}");
        assert!(state.config_view().await.has_api_key);

        // Second call has nothing left to take.
        assert_eq!(state.take_legacy_api_key().await, None);
    }

    #[tokio::test]
    async fn backend_config_and_runtime_stay_in_step() {
        let state = state().await;
        let backend = LocalBackendConfig {
            name: "probe".into(),
            transport: "stdio".into(),
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), "sleep 30".into()],
            ..Default::default()
        };

        state.add_backend(backend).await.unwrap();
        assert_eq!(config::load(&state.config_path).unwrap().backends.len(), 1);
        assert_eq!(state.snapshot().await.backends.len(), 1);

        state.set_backend_enabled("probe", false).await.unwrap();
        assert!(!config::load(&state.config_path).unwrap().backends[0].enabled);

        state.remove_backend("probe").await.unwrap();
        assert!(config::load(&state.config_path)
            .unwrap()
            .backends
            .is_empty());
        assert!(state.snapshot().await.backends.is_empty());
    }

    #[tokio::test]
    async fn adding_a_backend_that_fails_validation_does_not_touch_the_config() {
        let state = state().await;
        let err = state
            .add_backend(LocalBackendConfig {
                name: "bad".into(),
                transport: "stdio".into(),
                command: None,
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(err.contains("command"), "{err}");
        assert!(state.config().await.backends.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_changes_collapses_into_one_register() {
        let state = state().await;
        // No writer is installed, so `send_register` returns false; what is
        // being asserted here is the *timing* — that the debouncer waits out the
        // quiet period rather than firing per change.
        let (tx, mut rx) = mpsc::channel::<String>(16);
        state.set_writer(Some(tx)).await;

        for _ in 0..10 {
            state.request_reregister();
        }
        tokio::time::sleep(REREGISTER_DEBOUNCE * 3).await;

        let mut frames = 0;
        while rx.try_recv().is_ok() {
            frames += 1;
        }
        assert_eq!(frames, 1, "ten changes must produce one register frame");
    }
}

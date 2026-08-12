//! The on-disk configuration: `~/.mcp-gateway-agent/config.toml`.
//!
//! Two things are worth knowing before editing this file.
//!
//! **The API key does not belong here any more.** It is stored in the macOS
//! Keychain. `AgentConfig::api_key` exists only so that a config written by the
//! old terminal agent can still be read, and so first launch can migrate the
//! value across; once migrated the field is dropped from the file (it is
//! `skip_serializing_if = "Option::is_none"`). Nothing in this struct is sent to
//! the webview directly — see [`ConfigView`].
//!
//! **Writes are atomic.** [`save`] writes a sibling temp file with mode 0600 and
//! renames it into place, so an interrupted write can never leave a half-parsed
//! config (and with it, an agent that will not start) behind.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

/// Stands in for a masked value everywhere the value itself is not allowed to
/// go — the snapshot the app renders, the sheet you edit a backend in, a
/// `test_backend` payload on its way back down.
///
/// A masked variable is still stored in plain text in `config.toml`; masking is
/// a rule about what may be *displayed*, not encryption. The editor hands this
/// placeholder back for any variable the user did not retype, and
/// [`LocalBackendConfig::restore_masked_from`] swaps the real value in again
/// before the config is written or a process is started.
///
/// The same string is spelled out in `mcp-gateway-server/src/api/backends.rs`;
/// the two crates cannot share a constant, so if you change it here, change it
/// there in the same commit.
pub const MASKED: &str = "__mcpgw_masked__";

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub backends: Vec<LocalBackendConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct AgentConfig {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub gateway_url: String,
    /// Legacy plaintext credential, read once and migrated into the Keychain.
    /// Never serialize this back out once it is `None`, and never put it in an
    /// IPC payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Dashboard / REST base URL (e.g. `https://mcp-gateway.example.com`). Used
    /// by the Audit and Usage pages. Derived from `gateway_url` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
    /// Skip TLS certificate verification — for self-signed gateways only.
    #[serde(default)]
    pub tls_skip_verify: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LocalBackendConfig {
    pub name: String,
    /// `stdio`, `http`, or `streamable-http`.
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Keys of `env` whose values must never be shown again — not in this app,
    /// not on the dashboard. Everything not listed here is plain text, which is
    /// the default: most of what goes in an env block is a path or a flag, and
    /// hiding all of it taught people nothing about which ones were secret.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masked_env: Vec<String>,
    /// The same, for `headers`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masked_headers: Vec<String>,
    /// A disabled backend stays in the config but is never spawned and never
    /// registered with the gateway.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for LocalBackendConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: "stdio".into(),
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            masked_env: Vec::new(),
            masked_headers: Vec::new(),
            enabled: true,
        }
    }
}

impl LocalBackendConfig {
    pub fn is_stdio(&self) -> bool {
        self.transport == "stdio"
    }

    pub fn is_http(&self) -> bool {
        matches!(self.transport.as_str(), "http" | "streamable-http")
    }

    /// Put the real values back where the editor sent [`MASKED`].
    ///
    /// The sheet is never given a masked value, so this is what lets a backend
    /// with a secret in its environment survive an edit that only changed its
    /// arguments. `previous` is the configuration currently on disk; for a
    /// backend being added it is `Default`, and an unresolvable placeholder
    /// collapses to an empty value rather than being stored literally.
    pub fn restore_masked_from(&mut self, previous: &Self) {
        for (key, value) in self.env.iter_mut() {
            if value == MASKED {
                *value = previous.env.get(key).cloned().unwrap_or_default();
            }
        }
        for (key, value) in self.headers.iter_mut() {
            if value == MASKED {
                *value = previous.headers.get(key).cloned().unwrap_or_default();
            }
        }
        self.tidy_masks();
    }

    /// Sort the mask lists and drop entries for keys that no longer exist, so a
    /// renamed or deleted variable cannot leave a flag behind that would mask a
    /// future variable of the same name.
    pub fn tidy_masks(&mut self) {
        let env = &self.env;
        self.masked_env.retain(|k| env.contains_key(k));
        self.masked_env.sort();
        self.masked_env.dedup();

        let headers = &self.headers;
        self.masked_headers.retain(|k| headers.contains_key(k));
        self.masked_headers.sort();
        self.masked_headers.dedup();
    }

    /// Reject a backend that can never start, before it is written to disk.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Backend name is required".into());
        }
        if self.name.contains("__") {
            // Tools are namespaced `backend__tool`; a name containing the
            // separator would make routing ambiguous.
            return Err("Backend name cannot contain '__'".into());
        }
        if self.name.contains(char::is_whitespace) {
            return Err("Backend name cannot contain spaces".into());
        }
        if self.is_stdio() {
            if self.command.as_deref().unwrap_or("").trim().is_empty() {
                return Err("A stdio backend needs a command".into());
            }
        } else if self.is_http() {
            let url = self.url.as_deref().unwrap_or("").trim();
            if url.is_empty() {
                return Err("An HTTP backend needs a URL".into());
            }
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err("Backend URL must start with http:// or https://".into());
            }
        } else {
            return Err(format!("Unknown transport '{}'", self.transport));
        }
        Ok(())
    }
}

impl Config {
    /// True once the agent has enough to connect. The first-run wizard runs
    /// until this holds.
    pub fn is_configured(&self) -> bool {
        !self.agent.agent_id.trim().is_empty() && !self.agent.gateway_url.trim().is_empty()
    }

    pub fn backend(&self, name: &str) -> Option<&LocalBackendConfig> {
        self.backends.iter().find(|b| b.name == name)
    }

    /// REST base URL for the Audit and Usage pages.
    ///
    /// `gateway_url` is a WebSocket endpoint like
    /// `wss://host/agent/ws`; the REST API lives at
    /// `https://host/api/v1`. Derive one from the other unless the user set
    /// `dashboard_url` explicitly.
    pub fn api_base_url(&self) -> Option<String> {
        if let Some(url) = self.agent.dashboard_url.as_deref() {
            let url = url.trim().trim_end_matches('/');
            if !url.is_empty() {
                return Some(format!("{url}/api/v1"));
            }
        }
        let ws = self.agent.gateway_url.trim();
        if ws.is_empty() {
            return None;
        }
        let http = if let Some(rest) = ws.strip_prefix("wss://") {
            format!("https://{rest}")
        } else if let Some(rest) = ws.strip_prefix("ws://") {
            format!("http://{rest}")
        } else {
            ws.to_string()
        };
        // Everything up to and including `/api/v1`, or the bare origin if the
        // URL does not contain it.
        match http.find("/api/v1") {
            Some(idx) => Some(http[..idx + "/api/v1".len()].to_string()),
            None => {
                let scheme_end = http.find("://").map(|i| i + 3).unwrap_or(0);
                let origin_end = http[scheme_end..]
                    .find('/')
                    .map(|i| scheme_end + i)
                    .unwrap_or(http.len());
                Some(format!("{}/api/v1", &http[..origin_end]))
            }
        }
    }
}

/// Where the gateway serves the agent WebSocket.
///
/// Top level, beside `/api/v1` rather than under it — see the route table in
/// `mcp-gateway-server/src/main.rs`.
pub const AGENT_WS_PATH: &str = "/agent/ws";

/// Turn whatever a person pastes into a gateway WebSocket endpoint.
///
/// After sign-in the app fills this in itself, so most people never see it. It
/// still has to cope with what someone types into Settings: the dashboard URL,
/// a bare hostname, or the full `wss://…/agent/ws` endpoint.
pub fn normalize_gateway_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    let with_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else {
        // No scheme at all. Assume TLS, because anything on the open internet
        // needs it — unless it is obviously a machine on this desk.
        let local = trimmed.starts_with("localhost")
            || trimmed.starts_with("127.0.0.1")
            || trimmed.starts_with("[::1]");
        format!("{}{trimmed}", if local { "ws://" } else { "wss://" })
    };

    if with_scheme.ends_with(AGENT_WS_PATH) {
        return with_scheme;
    }
    let base = with_scheme
        .trim_end_matches('/')
        .trim_end_matches("/api/v1");
    format!("{base}{AGENT_WS_PATH}")
}

// ── Config as the app sees it ───────────────────────────────────────────

/// The config, minus the credential. This is the only shape that crosses the
/// IPC boundary: `has_api_key` tells the Settings page whether a key is stored
/// without ever handing the key itself to JavaScript.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigView {
    pub agent_id: String,
    pub gateway_url: String,
    pub dashboard_url: Option<String>,
    pub api_base_url: Option<String>,
    pub tls_skip_verify: bool,
    pub has_api_key: bool,
    pub configured: bool,
    pub config_path: String,
}

impl ConfigView {
    pub fn new(config: &Config, path: &Path, has_api_key: bool) -> Self {
        Self {
            agent_id: config.agent.agent_id.clone(),
            gateway_url: config.agent.gateway_url.clone(),
            dashboard_url: config.agent.dashboard_url.clone(),
            api_base_url: config.api_base_url(),
            tls_skip_verify: config.agent.tls_skip_verify,
            has_api_key,
            configured: config.is_configured() && has_api_key,
            config_path: path.display().to_string(),
        }
    }
}

// ── Paths ───────────────────────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mcp-gateway-agent")
}

pub fn default_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn logs_dir() -> PathBuf {
    config_dir().join("logs")
}

/// Where the old terminal agent installed its binary. Kept so the migration
/// step on first launch can offer to remove it.
pub fn legacy_bin_dir() -> PathBuf {
    config_dir().join("bin")
}

/// PID file written by the old `mcp-gateway-agent run`.
pub fn legacy_pid_file() -> PathBuf {
    config_dir().join("agent.pid")
}

/// Create the config directory owner-only. The directory holds logs that can
/// contain backend output, so other local users have no business reading it.
pub fn ensure_dirs() -> anyhow::Result<()> {
    for dir in [config_dir(), logs_dir()] {
        std::fs::create_dir_all(&dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(config_dir(), std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

// ── Load / save ─────────────────────────────────────────────────────────

/// Read the config. A missing file is not an error — it means "first run", and
/// the wizard takes over.
pub fn load(path: &Path) -> anyhow::Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("{} is not valid TOML: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(anyhow::anyhow!("Cannot read {}: {e}", path.display())),
    }
}

/// Write the config atomically, owner-only.
///
/// Temp file in the same directory (so the rename cannot cross filesystems),
/// mode 0600 set *before* anything is written into it, then `rename`. A crash
/// mid-write leaves the previous config intact rather than a truncated one.
pub fn save(path: &Path, config: &Config) -> anyhow::Result<()> {
    let text = toml::to_string_pretty(config)?;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;

    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));

    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        Config {
            agent: AgentConfig {
                agent_id: "sids-macbook-pro".into(),
                gateway_url: "wss://gw.example.com/agent/ws".into(),
                api_key: None,
                dashboard_url: None,
                tls_skip_verify: false,
            },
            backends: vec![LocalBackendConfig {
                name: "obsidian".into(),
                transport: "http".into(),
                url: Some("http://127.0.0.1:3010/mcp".into()),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = sample();
        save(&path, &original).unwrap();
        assert_eq!(load(&path).unwrap(), original);
    }

    #[test]
    fn saved_config_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save(&path, &sample()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "config must not be group/world readable"
            );
        }
    }

    #[test]
    fn save_leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save(&path, &sample()).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["config.toml".to_string()]);
    }

    #[test]
    fn missing_file_is_first_run_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = load(&dir.path().join("nope.toml")).unwrap();
        assert!(!config.is_configured());
    }

    #[test]
    fn reads_a_config_written_by_the_old_terminal_agent() {
        // The exact shape `mcp-gateway-agent setup` used to write: a plaintext
        // api_key, no `enabled` on the backends.
        let legacy = r#"
[agent]
agent_id = "sids-macbook-pro"
gateway_url = "wss://gw.example.com/agent/ws"
api_key = "mcpgw_abcdefghijklmnop"
tls_skip_verify = false

[[backends]]
name = "blender"
transport = "stdio"
command = "uvx"
args = ["blender-mcp"]
"#;
        let config: Config = toml::from_str(legacy).unwrap();
        assert_eq!(
            config.agent.api_key.as_deref(),
            Some("mcpgw_abcdefghijklmnop")
        );
        assert!(config.backends[0].enabled, "backends default to enabled");
    }

    #[test]
    fn migrating_the_key_drops_it_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = sample();
        config.agent.api_key = Some("mcpgw_secret_value_here".into());
        save(&path, &config).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("mcpgw_"));

        // What the Keychain migration does: take the key, save again.
        let migrated = config.agent.api_key.take();
        assert_eq!(migrated.as_deref(), Some("mcpgw_secret_value_here"));
        save(&path, &config).unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("api_key"),
            "the key must be gone from the file after migration:\n{on_disk}"
        );
        assert_eq!(load(&path).unwrap().agent.api_key, None);
    }

    #[test]
    fn derives_the_rest_base_from_the_websocket_url() {
        let mut config = sample();
        assert_eq!(
            config.api_base_url().as_deref(),
            Some("https://gw.example.com/api/v1")
        );

        config.agent.gateway_url = "ws://localhost:8080/agent/ws".into();
        assert_eq!(
            config.api_base_url().as_deref(),
            Some("http://localhost:8080/api/v1")
        );

        // No /api/v1 in the path at all.
        config.agent.gateway_url = "wss://gw.example.com/agent".into();
        assert_eq!(
            config.api_base_url().as_deref(),
            Some("https://gw.example.com/api/v1")
        );

        // An explicit dashboard_url wins.
        config.agent.dashboard_url = Some("https://dash.example.com/".into());
        assert_eq!(
            config.api_base_url().as_deref(),
            Some("https://dash.example.com/api/v1")
        );
    }

    #[test]
    fn rejects_backends_that_could_never_start() {
        let stdio_without_command = LocalBackendConfig {
            name: "x".into(),
            transport: "stdio".into(),
            ..Default::default()
        };
        assert!(stdio_without_command.validate().is_err());

        let namespace_clash = LocalBackendConfig {
            name: "my__backend".into(),
            transport: "stdio".into(),
            command: Some("echo".into()),
            ..Default::default()
        };
        assert!(namespace_clash.validate().is_err());

        let http_without_scheme = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            url: Some("127.0.0.1:3010".into()),
            ..Default::default()
        };
        assert!(http_without_scheme.validate().is_err());

        let good = LocalBackendConfig {
            name: "obsidian".into(),
            transport: "http".into(),
            url: Some("http://127.0.0.1:3010/mcp".into()),
            ..Default::default()
        };
        assert!(good.validate().is_ok());
    }

    fn with_secret() -> LocalBackendConfig {
        LocalBackendConfig {
            name: "gitea".into(),
            transport: "stdio".into(),
            command: Some("gitea-mcp".into()),
            env: HashMap::from([
                ("GITEA_TOKEN".into(), "the-real-token".into()),
                ("GITEA_URL".into(), "http://gitea.local".into()),
            ]),
            masked_env: vec!["GITEA_TOKEN".into()],
            ..Default::default()
        }
    }

    #[test]
    fn an_edit_that_does_not_retype_a_secret_keeps_it() {
        // What the sheet sends back: the placeholder for the masked variable,
        // because it was never given the value to begin with.
        let mut edited = with_secret();
        edited.env.insert("GITEA_TOKEN".into(), MASKED.into());
        edited
            .env
            .insert("GITEA_URL".into(), "http://new.local".into());

        edited.restore_masked_from(&with_secret());

        assert_eq!(edited.env["GITEA_TOKEN"], "the-real-token");
        assert_eq!(edited.env["GITEA_URL"], "http://new.local");
        assert_eq!(edited.masked_env, vec!["GITEA_TOKEN".to_string()]);
    }

    #[test]
    fn unmasking_keeps_the_value_it_was_hiding() {
        // Clearing the flag is the only way back to a masked value: the sheet
        // still sends the placeholder, the value is restored, and from the next
        // snapshot on it is shown in plain text.
        let mut edited = with_secret();
        edited.env.insert("GITEA_TOKEN".into(), MASKED.into());
        edited.masked_env.clear();

        edited.restore_masked_from(&with_secret());

        assert_eq!(edited.env["GITEA_TOKEN"], "the-real-token");
        assert!(edited.masked_env.is_empty());
    }

    #[test]
    fn a_placeholder_with_nothing_behind_it_does_not_become_the_value() {
        // Adding a backend: there is no previous config to restore from, and
        // storing the literal placeholder would hand the backend a nonsense
        // environment.
        let mut fresh = with_secret();
        fresh.env.insert("GITEA_TOKEN".into(), MASKED.into());
        fresh.restore_masked_from(&LocalBackendConfig::default());
        assert_eq!(fresh.env["GITEA_TOKEN"], "");
    }

    #[test]
    fn a_removed_variable_takes_its_mask_with_it() {
        let mut config = with_secret();
        config.env.remove("GITEA_TOKEN");
        config.tidy_masks();
        assert!(config.masked_env.is_empty());
    }

    #[test]
    fn masks_survive_the_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut original = sample();
        original.backends = vec![with_secret()];
        save(&path, &original).unwrap();
        assert_eq!(load(&path).unwrap(), original);
    }

    #[test]
    fn normalizes_the_three_things_people_actually_paste() {
        let endpoint = "wss://gw.example.com/agent/ws";
        // The full endpoint, unchanged.
        assert_eq!(normalize_gateway_url(endpoint), endpoint);
        // The dashboard URL.
        assert_eq!(normalize_gateway_url("https://gw.example.com"), endpoint);
        assert_eq!(normalize_gateway_url("https://gw.example.com/"), endpoint);
        assert_eq!(
            normalize_gateway_url("https://gw.example.com/api/v1"),
            endpoint
        );
        // A bare hostname.
        assert_eq!(normalize_gateway_url("gw.example.com"), endpoint);
        // Local development stays on plain ws.
        assert_eq!(
            normalize_gateway_url("localhost:8080"),
            "ws://localhost:8080/agent/ws"
        );
        assert_eq!(
            normalize_gateway_url("http://127.0.0.1:8080"),
            "ws://127.0.0.1:8080/agent/ws"
        );
        assert_eq!(normalize_gateway_url("   "), "");
    }

    #[test]
    fn config_view_never_carries_the_key() {
        let mut config = sample();
        config.agent.api_key = Some("mcpgw_supersecret".into());
        let view = ConfigView::new(&config, Path::new("/tmp/config.toml"), true);
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("mcpgw_supersecret"),
            "key leaked to the webview: {json}"
        );
        assert!(json.contains("\"has_api_key\":true"));
    }
}

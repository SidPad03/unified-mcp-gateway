use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub agent: AgentConfig,
    #[serde(default)]
    pub backends: Vec<LocalBackendConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentConfig {
    pub agent_id: String,
    pub gateway_url: String,
    pub api_key: String,
    /// Dashboard/API base URL (e.g., https://mcp-gateway.example.com) used for updates.
    /// If not set, derived from gateway_url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
    /// Skip TLS certificate verification (for self-signed certs)
    #[serde(default)]
    pub tls_skip_verify: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LocalBackendConfig {
    pub name: String,
    pub transport: String,
    /// Command to spawn (stdio transport)
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments for the command (stdio transport)
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the command (stdio transport)
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// URL for HTTP MCP backends
    #[serde(default)]
    pub url: Option<String>,
    /// Custom headers for HTTP backends (e.g., Authorization)
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mcp-gateway-agent")
}

pub fn default_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn bin_dir() -> PathBuf {
    config_dir().join("bin")
}

pub fn logs_dir() -> PathBuf {
    config_dir().join("logs")
}

pub fn cache_dir() -> PathBuf {
    config_dir().join("cache")
}

pub fn pid_file() -> PathBuf {
    config_dir().join("agent.pid")
}

pub fn ensure_dirs() -> anyhow::Result<()> {
    for dir in [config_dir(), bin_dir(), logs_dir(), cache_dir()] {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct McpGatewayConfig {
    pub gateway: GatewayConfig,
    pub storage: StorageConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub backends: Vec<BackendConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default = "default_dashboard_port")]
    pub dashboard_port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_retention_days")]
    pub audit_retention_days: u32,
    #[serde(default)]
    pub full_capture: bool,
    #[serde(default)]
    pub encryption_key_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub jwt_secret: String,
    #[serde(default = "default_token_expiry")]
    pub token_expiry_hours: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackendConfig {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_risk_category")]
    pub risk_category: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_health_interval")]
    pub health_check_interval_secs: u32,
    #[serde(default = "default_restart_policy")]
    pub restart_policy: String,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

fn default_listen_addr() -> String { "127.0.0.1:3100".into() }
fn default_transport() -> String { "streamable-http".into() }
fn default_dashboard_port() -> u16 { 3200 }
fn default_log_level() -> String { "info".into() }
fn default_db_path() -> String { "~/.mcp-gateway/mcp-gateway.db".into() }
fn default_retention_days() -> u32 { 90 }
fn default_true() -> bool { true }
fn default_token_expiry() -> u32 { 24 }
fn default_risk_category() -> String { "read".into() }
fn default_health_interval() -> u32 { 30 }
fn default_restart_policy() -> String { "on-failure".into() }
fn default_max_restarts() -> u32 { 3 }

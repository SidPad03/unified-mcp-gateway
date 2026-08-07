//! Core of the MCP Gateway Agent.
//!
//! Everything the agent actually *does* lives here: reading its config, running
//! the local MCP backends, and keeping the WebSocket tunnel to the gateway up.
//! There is deliberately no GUI dependency in this crate — the Tauri app in
//! `../src-tauri` is a thin shell over this API, and CI can run fmt / clippy /
//! test against this crate on a plain Linux runner.
//!
//! The shape of the thing:
//!
//! ```text
//!            ┌──────────────┐  tools/call   ┌────────────────┐
//!  gateway ──┤    Tunnel    ├──────────────►│ BackendManager │
//!   (WS)     └──────┬───────┘               └───────┬────────┘
//!                   │ status/logs/calls             │ one Supervisor per backend
//!                   ▼                               ▼
//!            ┌──────────────────────────────────────────────┐
//!            │  AgentState — LogBuffer, CallBuffer, config   │
//!            └──────────────────────────────────────────────┘
//! ```
//!
//! Timestamps crossing the boundary are RFC 3339 in **UTC**; the webview
//! formats them for the local timezone. (The old TUI computed
//! `secs % 86400` on the UNIX epoch and rendered UTC time-of-day as if it were
//! local — defect #7 in the design doc.)

pub mod backends;
pub mod config;
pub mod logbuf;
pub mod protocol;
pub mod redact;
pub mod state;
pub mod supervisor;
pub mod tunnel;

pub use config::{AgentConfig, Config, LocalBackendConfig};
pub use logbuf::{CallBuffer, CallStatus, LogBuffer, LogLevel, LogLine, ToolCall};
pub use protocol::{AgentMessage, GatewayMessage, SubBackendInfo, ToolInfo};
pub use state::{AgentState, ConnState, ConnectionStatus};

/// Version of the core crate, surfaced in the About panel and in the MCP
/// `clientInfo` we send to local backends.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format a UTC instant the way every timestamp crosses the IPC boundary.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

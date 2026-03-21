use std::fmt;

#[derive(Debug, Clone)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting(u32),
    Disconnected(String),
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting => write!(f, "Connecting..."),
            Self::Connected => write!(f, "Connected"),
            Self::Reconnecting(n) => write!(f, "Reconnecting (attempt {})", n),
            Self::Disconnected(reason) => write!(f, "Disconnected: {}", reason),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    ConnectionStatus(ConnectionState),
    Registered {
        backend_id: String,
    },
    ToolCallReceived {
        request_id: String,
        tool: String,
    },
    ToolCallCompleted {
        request_id: String,
        tool: String,
        duration_ms: u64,
        success: bool,
    },
    Log {
        level: LogLevel,
        message: String,
    },
    UpdateAvailable {
        version: String,
    },
    BackendStarted {
        name: String,
        transport: String,
        tool_count: usize,
    },
}

#[derive(Debug, Clone)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

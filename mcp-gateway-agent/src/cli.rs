use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mcp-gateway-agent", version, about = "MCP Gateway Agent")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the agent in the background
    Run {
        /// Run in the foreground instead of daemonizing (for service managers / debugging)
        #[arg(long)]
        foreground: bool,

        /// Path to config file (default: ~/.mcp-gateway-agent/config.toml)
        #[arg(long, short)]
        config: Option<String>,
    },
    /// Stop the running agent
    Stop,
    /// Restart the agent (stop + run)
    Restart {
        /// Path to config file (default: ~/.mcp-gateway-agent/config.toml)
        #[arg(long, short)]
        config: Option<String>,
    },
    /// Open the live TUI dashboard
    Dashboard {
        /// Path to config file (default: ~/.mcp-gateway-agent/config.toml)
        #[arg(long, short)]
        config: Option<String>,
    },
    /// Interactive setup wizard
    Setup,
    /// Check for and install updates
    Update {
        /// Only check, don't install
        #[arg(long)]
        check_only: bool,

        /// Auto-confirm update without prompting
        #[arg(long, short)]
        yes: bool,
    },
    /// Manage background service (launchd/systemd/Task Scheduler)
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Tail agent logs
    Logs {
        /// Number of lines to show initially
        #[arg(long, short, default_value = "50")]
        lines: u32,
    },
    /// Uninstall the agent (removes config, binary, and service)
    Uninstall,
    /// Show version information
    Version,
}

#[derive(Subcommand)]
pub enum ServiceAction {
    /// Install and start the background service
    Install,
    /// Stop and remove the background service
    Uninstall,
    /// Show service status
    Status,
    /// Tail service logs
    Logs,
}

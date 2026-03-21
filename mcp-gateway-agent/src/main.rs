use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cli;
mod config;
mod local_backends;
mod service;
mod tui;
mod tunnel;
mod update;

use cli::{Cli, Commands, ServiceAction};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { foreground, config } => {
            if foreground {
                run_foreground(config).await
            } else {
                run_daemonized(config)
            }
        }
        Commands::Stop => stop_agent(),
        Commands::Restart { config } => {
            let _ = stop_agent();
            // Brief pause to let the old process exit
            std::thread::sleep(std::time::Duration::from_millis(500));
            run_daemonized(config)
        }
        Commands::Dashboard { config } => run_dashboard(config).await,
        Commands::Setup => run_setup().await,
        Commands::Update { check_only, yes } => {
            init_logging();
            update::run_update_command(check_only, yes).await
        }
        Commands::Service { action } => {
            match action {
                ServiceAction::Install => service::install(),
                ServiceAction::Uninstall => service::uninstall(),
                ServiceAction::Status => service::status(),
                ServiceAction::Logs => service::logs(),
            }
        }
        Commands::Logs { lines } => run_logs(lines),
        Commands::Uninstall => run_uninstall(),
        Commands::Version => {
            println!("mcp-gateway-agent v{}", env!("CARGO_PKG_VERSION"));
            println!("arch: {}", update::current_arch());
            Ok(())
        }
    }
}

fn init_logging() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "mcp_gateway_agent=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

// ---------------------------------------------------------------------------
// Process management helpers
// ---------------------------------------------------------------------------

fn write_pid_file() -> anyhow::Result<()> {
    let pid = std::process::id();
    std::fs::write(config::pid_file(), pid.to_string())?;
    Ok(())
}

fn read_pid_file() -> Option<u32> {
    std::fs::read_to_string(config::pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn remove_pid_file() {
    let _ = std::fs::remove_file(config::pid_file());
}

/// Check if a process with the given PID is still running.
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks if process exists without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
}

/// Kill the process with the given PID.
fn kill_process(pid: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if ret != 0 {
            anyhow::bail!("Failed to send SIGTERM to process {}", pid);
        }
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to kill process {}", pid);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// run (daemonized) — spawns self as a background process
// ---------------------------------------------------------------------------

fn run_daemonized(config_path: Option<String>) -> anyhow::Result<()> {
    // Check if already running
    if let Some(pid) = read_pid_file() {
        if is_process_running(pid) {
            println!("Agent is already running (PID {}). Use 'restart' to restart.", pid);
            return Ok(());
        }
        // Stale PID file
        remove_pid_file();
    }

    config::ensure_dirs()?;

    let exe = std::env::current_exe()?;
    let logs = config::logs_dir();
    let stdout_path = logs.join("agent.stdout.log");
    let stderr_path = logs.join("agent.stderr.log");

    let stdout_file = std::fs::OpenOptions::new()
        .create(true).append(true).open(&stdout_path)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true).append(true).open(&stderr_path)?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("run").arg("--foreground");
    if let Some(ref cfg) = config_path {
        cmd.arg("--config").arg(cfg);
    }
    cmd.stdout(stdout_file).stderr(stderr_file);

    // Detach from the current terminal
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn()?;
    let pid = child.id();

    std::fs::write(config::pid_file(), pid.to_string())?;

    println!("Agent started in background (PID {}).", pid);
    println!("  Logs: {}", stdout_path.display());
    println!("  Stop: mcp-gateway-agent stop");
    Ok(())
}

// ---------------------------------------------------------------------------
// run --foreground — runs headlessly in the current process
// ---------------------------------------------------------------------------

async fn run_foreground(config_path: Option<String>) -> anyhow::Result<()> {
    init_logging();
    write_pid_file()?;

    let config = load_config(config_path)?;

    let mut manager = local_backends::LocalBackendManager::new();
    manager.start_all(&config.backends).await?;

    let tool_count = manager.all_tools().len();
    tracing::info!(tool_count, "All local backends started, tools discovered");

    if tool_count == 0 {
        tracing::warn!("No tools discovered from local backends. The agent will connect but register zero tools.");
    }

    for tool in manager.all_tools() {
        tracing::info!(tool = %tool.name, "  Discovered tool");
    }

    let manager = Arc::new(manager);

    tracing::info!("Starting gateway tunnel (Ctrl-C to stop)...");

    tokio::select! {
        _ = tunnel::run_tunnel(&config, manager, None) => {}
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received Ctrl-C, shutting down...");
        }
    }

    remove_pid_file();
    Ok(())
}

// ---------------------------------------------------------------------------
// stop
// ---------------------------------------------------------------------------

fn stop_agent() -> anyhow::Result<()> {
    match read_pid_file() {
        Some(pid) if is_process_running(pid) => {
            kill_process(pid)?;
            remove_pid_file();
            println!("Agent stopped (PID {}).", pid);
            Ok(())
        }
        Some(_) => {
            remove_pid_file();
            println!("Agent was not running (stale PID file removed).");
            Ok(())
        }
        None => {
            println!("Agent is not running (no PID file found).");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// dashboard — TUI with live tunnel
// ---------------------------------------------------------------------------

async fn run_dashboard(config_path: Option<String>) -> anyhow::Result<()> {
    let config = load_config(config_path)?;

    let mut manager = local_backends::LocalBackendManager::new();
    manager.start_all(&config.backends).await?;
    let manager = Arc::new(manager);

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

    // Emit initial backend info
    for backend in &config.backends {
        let tool_count = manager.all_tools().iter()
            .filter(|t| t.name.starts_with(&format!("{}_", backend.name)))
            .count();
        let _ = event_tx.send(tui::events::AgentEvent::BackendStarted {
            name: backend.name.clone(),
            transport: backend.transport.clone(),
            tool_count,
        });
    }

    // Spawn the tunnel in background
    let tunnel_config = config;
    let tunnel_manager = manager;
    let tunnel_events = event_tx.clone();
    tokio::spawn(async move {
        tunnel::run_tunnel(&tunnel_config, tunnel_manager, Some(tunnel_events)).await;
    });

    // Run the TUI dashboard
    tui::run_dashboard(event_rx).await
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn load_config(config_path: Option<String>) -> anyhow::Result<config::Config> {
    let config_path = config_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(config::default_config_path);

    let config_content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!(
            "Failed to read config file '{}': {}. Run 'mcp-gateway-agent setup' to create one.",
            config_path.display(),
            e
        ))?;

    let config: config::Config = toml::from_str(&config_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    Ok(config)
}

fn run_logs(lines: u32) -> anyhow::Result<()> {
    let logs_dir = config::logs_dir();
    let stdout_log = logs_dir.join("agent.stdout.log");
    let stderr_log = logs_dir.join("agent.stderr.log");

    if !stdout_log.exists() && !stderr_log.exists() {
        println!("No log files found at {}", logs_dir.display());
        println!("Logs are created when the agent runs in the background.");
        println!("Start the agent with: mcp-gateway-agent run");
        return Ok(());
    }

    println!("Tailing logs (Ctrl-C to stop)...");
    println!("  stdout: {}", stdout_log.display());
    println!("  stderr: {}", stderr_log.display());
    println!("---");

    #[cfg(unix)]
    {
        let mut args = vec!["-f".to_string(), "-n".to_string(), lines.to_string()];
        if stdout_log.exists() {
            args.push(stdout_log.to_string_lossy().to_string());
        }
        if stderr_log.exists() {
            args.push(stderr_log.to_string_lossy().to_string());
        }

        let status = std::process::Command::new("tail")
            .args(&args)
            .status()?;

        if !status.success() {
            eprintln!("tail exited with: {:?}", status.code());
        }
    }

    #[cfg(windows)]
    {
        let log_path = if stdout_log.exists() {
            &stdout_log
        } else {
            &stderr_log
        };
        let status = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Get-Content -Path '{}' -Tail {} -Wait",
                    log_path.display(),
                    lines
                ),
            ])
            .status()?;

        if !status.success() {
            eprintln!("Log tailing exited with: {:?}", status.code());
        }
    }

    Ok(())
}

fn run_uninstall() -> anyhow::Result<()> {
    use std::io::Write;

    let service_type = if cfg!(target_os = "macos") {
        "launchd service"
    } else if cfg!(target_os = "linux") {
        "systemd service"
    } else {
        "scheduled task"
    };

    println!("This will remove:");
    println!("  - Config, cache and logs: {}", config::config_dir().display());
    println!("  - Binary: {}", config::bin_dir().join("mcp-gateway-agent").display());
    println!("  - {} (if installed)", service_type);
    println!("  - PATH entry from shell config");
    print!("\nAre you sure? [y/N] ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Uninstall cancelled.");
        return Ok(());
    }

    // Stop running agent if any
    let _ = stop_agent();

    // Stop and remove service (ignore errors)
    let _ = service::uninstall();

    // Remove PATH entry from shell config files
    remove_path_from_shell_configs();

    // Remove the config directory (config, logs, cache)
    let config_dir = config::config_dir();
    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)?;
        println!("Removed {}", config_dir.display());
    }

    println!("\nMCP Gateway Agent has been uninstalled.");
    println!("The running binary will be removed once this process exits.");
    Ok(())
}

fn remove_path_from_shell_configs() {
    let home = dirs::home_dir().unwrap_or_default();
    let bin_dir_str = config::bin_dir().to_string_lossy().to_string();

    #[cfg(unix)]
    {
        for rc_name in &[".zshrc", ".bashrc", ".bash_profile", ".profile"] {
            let rc = home.join(rc_name);
            if rc.exists() {
                if let Ok(contents) = std::fs::read_to_string(&rc) {
                    if contents.contains(&bin_dir_str) {
                        let filtered: Vec<&str> = contents
                            .lines()
                            .filter(|line| {
                                !line.contains(&bin_dir_str) && line.trim() != "# MCP Gateway Agent"
                            })
                            .collect();
                        let _ = std::fs::write(&rc, filtered.join("\n") + "\n");
                        println!("Removed PATH entry from {}", rc.display());
                    }
                }
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    r#"$p = [Environment]::GetEnvironmentVariable('PATH','User'); if ($p -and $p.Contains('{}')) {{ $p = ($p -split ';' | Where-Object {{ $_ -ne '{}' }}) -join ';'; [Environment]::SetEnvironmentVariable('PATH',$p,'User'); Write-Host 'Removed from user PATH' }}"#,
                    bin_dir_str, bin_dir_str
                ),
            ])
            .status();
    }
}

async fn run_setup() -> anyhow::Result<()> {
    let (should_start, should_install_service) = tui::run_setup().await?;

    if should_install_service {
        service::install()?;
    } else if should_start {
        run_daemonized(None)?;
    }

    Ok(())
}

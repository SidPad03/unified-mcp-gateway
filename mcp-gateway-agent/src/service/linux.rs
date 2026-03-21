use std::path::PathBuf;

const SERVICE_NAME: &str = "mcp-gateway-agent";

fn systemd_user_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/systemd/user")
}

fn unit_path() -> PathBuf {
    systemd_user_dir().join(format!("{}.service", SERVICE_NAME))
}

fn generate_unit() -> String {
    let bin = super::agent_bin_path();
    let logs = crate::config::logs_dir();
    let stdout_log = logs.join("agent.stdout.log");
    let stderr_log = logs.join("agent.stderr.log");

    format!(
        r#"[Unit]
Description=MCP Gateway Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={bin} run --foreground
Restart=always
RestartSec=5
StandardOutput=append:{stdout}
StandardError=append:{stderr}
Environment=PATH=/usr/local/bin:/usr/bin:/bin

[Install]
WantedBy=default.target
"#,
        bin = bin,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    )
}

pub fn install() -> anyhow::Result<()> {
    crate::config::ensure_dirs()?;

    let unit = unit_path();
    let content = generate_unit();

    if let Some(parent) = unit.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&unit, content)?;
    println!("Wrote systemd unit to {}", unit.display());

    let status = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", SERVICE_NAME])
        .status()?;

    if status.success() {
        println!("Service installed and started.");
        println!("The agent will run in the background and start automatically on login.");

        // Enable lingering so the user service starts without login
        let _ = std::process::Command::new("loginctl")
            .args(["enable-linger"])
            .status();
    } else {
        anyhow::bail!("systemctl enable --now failed with exit code: {:?}", status.code());
    }

    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    let unit = unit_path();

    if unit.exists() {
        let status = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", SERVICE_NAME])
            .status()?;

        if !status.success() {
            eprintln!("Warning: systemctl disable returned non-zero exit code");
        }

        std::fs::remove_file(&unit)?;

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        println!("Service uninstalled.");
    } else {
        println!("Service is not installed.");
    }

    Ok(())
}

pub fn status() -> anyhow::Result<()> {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "status", SERVICE_NAME])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() || !stdout.is_empty() {
        println!("{}", stdout);
    } else {
        let unit = unit_path();
        if unit.exists() {
            println!("Service unit exists but may not be running.");
            println!("Run 'mcp-gateway-agent service install' to start it.");
        } else {
            println!("Service is not installed.");
        }
    }

    Ok(())
}

pub fn logs() -> anyhow::Result<()> {
    let stdout_log = crate::config::logs_dir().join("agent.stdout.log");
    let stderr_log = crate::config::logs_dir().join("agent.stderr.log");

    println!("Tailing logs (Ctrl-C to stop)...");
    println!("stdout: {}", stdout_log.display());
    println!("stderr: {}", stderr_log.display());
    println!("---");

    let status = std::process::Command::new("tail")
        .args(["-f", "-n", "50"])
        .arg(&stdout_log)
        .arg(&stderr_log)
        .status()?;

    if !status.success() {
        eprintln!("tail exited with: {:?}", status.code());
    }

    Ok(())
}

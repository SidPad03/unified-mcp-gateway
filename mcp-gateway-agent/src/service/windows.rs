const TASK_NAME: &str = "MCPGatewayAgent";

pub fn install() -> anyhow::Result<()> {
    crate::config::ensure_dirs()?;

    let bin = super::agent_bin_path();
    let logs = crate::config::logs_dir();
    let stdout_log = logs.join("agent.stdout.log");

    // Create a scheduled task that runs at logon and restarts on failure
    let status = std::process::Command::new("schtasks")
        .args([
            "/Create",
            "/TN", TASK_NAME,
            "/TR", &format!("cmd /C \"{bin}\" run --foreground >> \"{}\" 2>&1", stdout_log.display()),
            "/SC", "ONLOGON",
            "/RL", "LIMITED",
            "/F",
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("schtasks /Create failed with exit code: {:?}", status.code());
    }

    // Start the task immediately
    let status = std::process::Command::new("schtasks")
        .args(["/Run", "/TN", TASK_NAME])
        .status()?;

    if status.success() {
        println!("Service installed and started via Task Scheduler.");
        println!("The agent will run in the background and start automatically on login.");
    } else {
        println!("Task created but failed to start immediately. It will start on next login.");
    }

    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    // Stop the task first (ignore errors if not running)
    let _ = std::process::Command::new("schtasks")
        .args(["/End", "/TN", TASK_NAME])
        .status();

    let status = std::process::Command::new("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .status()?;

    if status.success() {
        println!("Service uninstalled.");
    } else {
        println!("Service is not installed (or already removed).");
    }

    Ok(())
}

pub fn status() -> anyhow::Result<()> {
    let output = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/V", "/FO", "LIST"])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("{}", stdout);
    } else {
        println!("Service is not installed.");
    }

    Ok(())
}

pub fn logs() -> anyhow::Result<()> {
    let stdout_log = crate::config::logs_dir().join("agent.stdout.log");

    if !stdout_log.exists() {
        println!("No log files found at {}", crate::config::logs_dir().display());
        println!("Logs are created when the agent runs as a background service.");
        println!("Install the service with: mcp-gateway-agent service install");
        return Ok(());
    }

    println!("Log file: {}", stdout_log.display());
    println!("---");

    // Use PowerShell's Get-Content -Wait (equivalent to tail -f)
    let status = std::process::Command::new("powershell")
        .args([
            "-Command",
            &format!("Get-Content -Path '{}' -Tail 50 -Wait", stdout_log.display()),
        ])
        .status()?;

    if !status.success() {
        eprintln!("Log tailing exited with: {:?}", status.code());
    }

    Ok(())
}

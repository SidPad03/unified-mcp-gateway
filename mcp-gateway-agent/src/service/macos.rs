use std::path::PathBuf;

const PLIST_LABEL: &str = "com.mcpgateway.agent";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

fn generate_plist() -> String {
    let bin = super::agent_bin_path();
    let logs = crate::config::logs_dir();
    let stdout_log = logs.join("agent.stdout.log");
    let stderr_log = logs.join("agent.stderr.log");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>run</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin</string>
    </dict>
</dict>
</plist>"#,
        label = PLIST_LABEL,
        bin = bin,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    )
}

pub fn install() -> anyhow::Result<()> {
    crate::config::ensure_dirs()?;

    let plist = plist_path();
    let content = generate_plist();

    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&plist, content)?;
    println!("Wrote plist to {}", plist.display());

    let status = std::process::Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist)
        .status()?;

    if status.success() {
        println!("Service installed and started.");
        println!("The agent will run in the background and start automatically on login.");
    } else {
        anyhow::bail!("launchctl load failed with exit code: {:?}", status.code());
    }

    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    let plist = plist_path();

    if plist.exists() {
        let status = std::process::Command::new("launchctl")
            .args(["unload"])
            .arg(&plist)
            .status()?;

        if !status.success() {
            eprintln!("Warning: launchctl unload returned non-zero exit code");
        }

        std::fs::remove_file(&plist)?;
        println!("Service uninstalled.");
    } else {
        println!("Service is not installed.");
    }

    Ok(())
}

pub fn status() -> anyhow::Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Service is loaded:");
        println!("{}", stdout);
    } else {
        let plist = plist_path();
        if plist.exists() {
            println!("Service plist exists but is not loaded.");
            println!("Run 'mcp-gateway-agent service install' to load it.");
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

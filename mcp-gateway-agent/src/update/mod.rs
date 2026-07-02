use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AgentRelease {
    pub version: String,
    pub release_notes: String,
    pub published_at: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub arch: String,
    pub size: u64,
    pub checksum: Option<String>,
    pub download_url: Option<String>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn current_arch() -> &'static str {
    cfg_if::cfg_if! {
        if #[cfg(all(target_arch = "aarch64", target_os = "macos"))] {
            "aarch64-apple-darwin"
        } else if #[cfg(all(target_arch = "x86_64", target_os = "macos"))] {
            "x86_64-apple-darwin"
        } else if #[cfg(all(target_arch = "aarch64", target_os = "linux"))] {
            "aarch64-unknown-linux-gnu"
        } else if #[cfg(all(target_arch = "x86_64", target_os = "linux"))] {
            "x86_64-unknown-linux-gnu"
        } else if #[cfg(all(target_arch = "x86_64", target_os = "windows"))] {
            "x86_64-pc-windows-gnu"
        } else if #[cfg(all(target_arch = "aarch64", target_os = "windows"))] {
            "aarch64-pc-windows-gnu"
        } else {
            "unknown"
        }
    }
}

pub fn derive_http_url(gateway_ws_url: &str) -> String {
    let url = gateway_ws_url
        .replace("wss://", "https://")
        .replace("ws://", "http://");
    // Strip path (e.g., /agent/ws)
    if let Some(idx) = url.find("://") {
        let after_scheme = &url[idx + 3..];
        if let Some(slash_idx) = after_scheme.find('/') {
            return url[..idx + 3 + slash_idx].to_string();
        }
    }
    url
}

fn build_http_client(tls_skip_verify: bool) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(tls_skip_verify)
        .build()
}

pub async fn check_update(
    config: &crate::config::Config,
) -> anyhow::Result<Option<AgentRelease>> {
    let base_url = config.agent.dashboard_url.clone()
        .unwrap_or_else(|| derive_http_url(&config.agent.gateway_url));
    let url = format!("{}/api/v1/agent/releases/latest", base_url);

    let client = build_http_client(config.agent.tls_skip_verify)?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", &config.agent.api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Server returned HTTP {}", resp.status());
    }

    let release: AgentRelease = resp.json().await?;

    let current = semver::Version::parse(current_version())?;
    let latest = semver::Version::parse(&release.version)?;

    if latest > current {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

pub async fn download_and_apply(
    config: &crate::config::Config,
    release: &AgentRelease,
) -> anyhow::Result<()> {
    let base_url = config.agent.dashboard_url.clone()
        .unwrap_or_else(|| derive_http_url(&config.agent.gateway_url));
    let arch = current_arch();

    let asset = release
        .assets
        .iter()
        .find(|a| a.arch == arch)
        .ok_or_else(|| anyhow::anyhow!("No binary available for architecture {}", arch))?;

    let download_url = format!(
        "{}/api/v1/agent/releases/v{}/download?arch={}",
        base_url, release.version, arch
    );

    let client = build_http_client(config.agent.tls_skip_verify)?;
    let resp = client
        .get(&download_url)
        .header("Authorization", format!("Bearer {}", &config.agent.api_key))
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Download failed with HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;

    // Verify the SHA-256 checksum before we overwrite the running binary.
    // Releases always publish a checksums.sha256 (see CI), so a missing
    // checksum means a malformed or tampered release — fail closed rather than
    // installing an unverified binary.
    let expected_checksum = asset.checksum.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Release asset has no published checksum; refusing to install an unverified binary"
        )
    })?;
    {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if actual != *expected_checksum {
            anyhow::bail!(
                "Checksum mismatch: expected {}, got {}",
                expected_checksum,
                actual
            );
        }
    }

    // Write to cache
    let cache_name = if cfg!(windows) { "mcp-gateway-agent.new.exe" } else { "mcp-gateway-agent.new" };
    let cache_path = crate::config::cache_dir().join(cache_name);
    crate::config::ensure_dirs()?;
    std::fs::write(&cache_path, &bytes)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Atomic self-replacement
    self_replace::self_replace(&cache_path)?;

    // Clean up
    let _ = std::fs::remove_file(&cache_path);

    Ok(())
}

pub async fn run_update_command(check_only: bool, auto_yes: bool) -> anyhow::Result<()> {
    let config_path = crate::config::default_config_path();
    let config_content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("No config found at {}. Run 'mcp-gateway-agent setup' first. ({})", config_path.display(), e))?;
    let config: crate::config::Config = toml::from_str(&config_content)?;

    println!("Checking for updates...");
    println!("Current version: v{}", current_version());

    match check_update(&config).await? {
        None => {
            println!("You are running the latest version.");
            Ok(())
        }
        Some(release) => {
            println!("Update available: v{}", release.version);
            if !release.release_notes.is_empty() {
                println!("\nRelease notes:\n{}", release.release_notes);
            }

            if check_only {
                return Ok(());
            }

            if !auto_yes {
                print!("\nInstall update? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Update cancelled.");
                    return Ok(());
                }
            }

            println!("Downloading v{}...", release.version);
            download_and_apply(&config, &release).await?;
            println!("Updated successfully to v{}!", release.version);
            println!("Please restart the agent to use the new version.");
            Ok(())
        }
    }
}

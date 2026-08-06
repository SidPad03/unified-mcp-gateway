use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::api::auth::Claims;
use crate::{AppError, AppState};

const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentRelease {
    pub version: String,
    pub release_notes: String,
    pub published_at: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub arch: String,
    pub size: u64,
    pub checksum: Option<String>,
    #[serde(skip_serializing)]
    pub browser_download_url: String,
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    pub arch: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/releases", get(list_releases))
        .route("/agent/releases/latest", get(latest_release))
        .route("/agent/releases/:tag/download", get(download_release))
}

/// Base URL of the git forge that hosts agent releases, preferring the current
/// name and falling back to the legacy Gitea one.
///
/// Returns an error rather than panicking when neither is set. This used to be
/// an `.expect()`, which unwound the request handler: neither variable is
/// required at startup, and neither `.env.example` nor `docker-compose.yml`
/// sets one, so on a default deployment any authenticated GET of
/// `/api/v1/agent/releases` panicked instead of returning a response.
fn release_proxy_url() -> Result<String, AppError> {
    std::env::var("RELEASE_PROXY_URL")
        .or_else(|_| std::env::var("GITEA_URL"))
        .map_err(|_| {
            AppError::Internal(
                "Agent release proxy is not configured: set RELEASE_PROXY_URL (or the legacy \
                 GITEA_URL) on the server to enable the agent release endpoints."
                    .into(),
            )
        })
}

async fn fetch_releases(state: &AppState) -> Result<Vec<AgentRelease>, AppError> {
    // Check cache
    {
        let cache = state.agent_release_cache.lock().await;
        if let Some((cached_at, ref releases)) = *cache {
            if cached_at.elapsed() < CACHE_TTL {
                return Ok(releases.clone());
            }
        }
    }

    let gitea_url = release_proxy_url()?;
    let gitea_repo = std::env::var("RELEASE_PROXY_REPO")
        .or_else(|_| std::env::var("GITEA_AGENT_REPO"))
        .unwrap_or_else(|_| "SidPad03/unified-mcp-gateway".to_string());
    let gitea_token = std::env::var("GITEA_TOKEN").ok();

    let url = format!("{}/api/v1/repos/{}/releases", gitea_url, gitea_repo);

    let client = reqwest::Client::new();
    let mut req = client.get(&url);
    if let Some(ref token) = gitea_token {
        req = req.header("Authorization", format!("token {}", token));
    }

    let resp = req
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch releases from Gitea: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "Gitea returned HTTP {}",
            resp.status()
        )));
    }

    let gitea_releases: Vec<GiteaRelease> = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse Gitea releases: {}", e)))?;

    // Filter to agent releases (tags starting with "agent-v") and parse checksums
    let mut releases: Vec<AgentRelease> = Vec::new();
    for r in gitea_releases
        .into_iter()
        .filter(|r| r.tag_name.starts_with("agent-v"))
    {
        let version = r
            .tag_name
            .strip_prefix("agent-v")
            .unwrap_or(&r.tag_name)
            .to_string();

        // Fetch and parse checksums.sha256 if present in this release
        let checksums =
            if let Some(checksum_asset) = r.assets.iter().find(|a| a.name.contains("checksums")) {
                let mut req = client.get(&checksum_asset.browser_download_url);
                if let Some(ref token) = gitea_token {
                    req = req.header("Authorization", format!("token {}", token));
                }
                match req.timeout(Duration::from_secs(10)).send().await {
                    Ok(resp) if resp.status().is_success() => match resp.text().await {
                        Ok(text) => parse_checksums(&text),
                        Err(_) => std::collections::HashMap::new(),
                    },
                    _ => std::collections::HashMap::new(),
                }
            } else {
                std::collections::HashMap::new()
            };

        let assets = r
            .assets
            .into_iter()
            .filter_map(|a| {
                let arch = extract_arch(&a.name)?;
                let checksum = checksums.get(&a.name).cloned();
                Some(ReleaseAsset {
                    name: a.name,
                    arch,
                    size: a.size,
                    checksum,
                    browser_download_url: a.browser_download_url,
                })
            })
            .collect();

        releases.push(AgentRelease {
            version,
            release_notes: r.body,
            published_at: r.published_at,
            assets,
        });
    }

    // Update cache
    {
        let mut cache = state.agent_release_cache.lock().await;
        *cache = Some((Instant::now(), releases.clone()));
    }

    Ok(releases)
}

/// Parse a sha256sum-format file into a map of filename → hex digest.
/// Handles both "hash  filename" and "hash *filename" (binary mode) formats.
fn parse_checksums(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            // sha256sum format: "<hash>  <filename>" or "<hash> *<filename>"
            let (hash, rest) = line.split_once(|c: char| c.is_whitespace())?;
            let name = rest.trim().trim_start_matches('*').to_string();
            if hash.len() == 64 && !name.is_empty() {
                Some((name, hash.to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn extract_arch(filename: &str) -> Option<String> {
    const KNOWN_TARGETS: &[&str] = &[
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-gnu",
        "aarch64-pc-windows-gnu",
    ];
    for target in KNOWN_TARGETS {
        if filename.contains(target) {
            return Some(target.to_string());
        }
    }
    if filename.contains("checksums") {
        return None;
    }
    None
}

async fn list_releases(
    State(state): State<AppState>,
    _claims: Claims,
) -> Result<Json<Vec<AgentRelease>>, AppError> {
    let releases = fetch_releases(&state).await?;
    Ok(Json(releases))
}

async fn latest_release(
    State(state): State<AppState>,
    _claims: Claims,
) -> Result<Json<AgentRelease>, AppError> {
    let releases = fetch_releases(&state).await?;
    let latest = releases
        .into_iter()
        .max_by(|a, b| {
            let va = semver::Version::parse(&a.version).unwrap_or(semver::Version::new(0, 0, 0));
            let vb = semver::Version::parse(&b.version).unwrap_or(semver::Version::new(0, 0, 0));
            va.cmp(&vb)
        })
        .ok_or_else(|| AppError::NotFound("No agent releases found".into()))?;
    Ok(Json(latest))
}

async fn download_release(
    State(state): State<AppState>,
    _claims: Claims,
    Path(tag): Path<String>,
    Query(query): Query<DownloadQuery>,
) -> Result<impl IntoResponse, AppError> {
    let releases = fetch_releases(&state).await?;
    let version = tag.strip_prefix("v").unwrap_or(&tag);

    let release = releases
        .iter()
        .find(|r| r.version == version)
        .ok_or_else(|| AppError::NotFound(format!("Release {} not found", tag)))?;

    let asset = release
        .assets
        .iter()
        .find(|a| a.arch == query.arch)
        .ok_or_else(|| AppError::NotFound(format!("No asset for architecture {}", query.arch)))?;

    // Stream from Gitea
    let gitea_token = std::env::var("GITEA_TOKEN").ok();
    let client = reqwest::Client::new();
    let mut req = client.get(&asset.browser_download_url);
    if let Some(ref token) = gitea_token {
        req = req.header("Authorization", format!("token {}", token));
    }

    let resp = req
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to download from Gitea: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Internal(format!(
            "Gitea download returned HTTP {}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read download: {}", e)))?;

    let headers = [
        (
            axum::http::header::CONTENT_TYPE,
            "application/octet-stream".to_string(),
        ),
        (
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", asset.name),
        ),
    ];

    Ok((StatusCode::OK, headers, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;

    /// Regression test: an unconfigured release proxy must surface as an error,
    /// not a panic in the request handler. `.expect()` here took down the
    /// request on any default deployment, since neither variable is set by
    /// `.env.example` or `docker-compose.yml` and startup does not require them.
    #[test]
    fn unconfigured_release_proxy_errors_instead_of_panicking() {
        let _guard = lock_env();
        let saved_proxy = std::env::var("RELEASE_PROXY_URL").ok();
        let saved_legacy = std::env::var("GITEA_URL").ok();

        std::env::remove_var("RELEASE_PROXY_URL");
        std::env::remove_var("GITEA_URL");
        assert!(
            matches!(release_proxy_url(), Err(AppError::Internal(_))),
            "neither variable set should be a recoverable error"
        );

        // The legacy name is still honoured on its own.
        std::env::set_var("GITEA_URL", "https://forge.example.com");
        assert_eq!(release_proxy_url().unwrap(), "https://forge.example.com");

        // And the current name takes precedence over it.
        std::env::set_var("RELEASE_PROXY_URL", "https://proxy.example.com");
        assert_eq!(release_proxy_url().unwrap(), "https://proxy.example.com");

        match saved_proxy {
            Some(v) => std::env::set_var("RELEASE_PROXY_URL", v),
            None => std::env::remove_var("RELEASE_PROXY_URL"),
        }
        match saved_legacy {
            Some(v) => std::env::set_var("GITEA_URL", v),
            None => std::env::remove_var("GITEA_URL"),
        }
    }
}

// Gitea API types
#[derive(Deserialize)]
struct GiteaRelease {
    tag_name: String,
    body: String,
    published_at: String,
    assets: Vec<GiteaAsset>,
}

#[derive(Deserialize)]
struct GiteaAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

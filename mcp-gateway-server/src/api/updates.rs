//! Update availability check for the gateway + dashboard.
//!
//! # Why this is proxied through the server
//!
//! The dashboard cannot call `api.github.com` itself. Its nginx CSP pins
//! `connect-src` to `'self'`, the OpenAI API, and websockets, so a browser fetch
//! to GitHub is blocked outright. Routing through the server also means:
//!
//! - one cached upstream call serves every operator, instead of each browser
//!   spending from GitHub's 60-requests/hour unauthenticated IP budget;
//! - deployments behind an egress proxy configure it in one place;
//! - an optional `GITHUB_TOKEN` can raise the rate limit without shipping a
//!   credential to the browser.
//!
//! # Why the caller supplies its own version
//!
//! The release version is computed by CI and injected into the dashboard as a
//! build argument; the server binary is not rebuilt with it, so the server's own
//! `CARGO_PKG_VERSION` is the crate version and not the deployed release. The
//! server would therefore compare against the wrong number. The dashboard knows
//! exactly what it is running (`__APP_VERSION__`) and passes it in.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use super::auth::Claims;
use crate::{AppError, AppState};

/// Releases move rarely; an operator clicking the button repeatedly should not
/// generate upstream traffic.
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);
/// Never let a slow upstream hold a dashboard request open.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);
/// Gateway releases are tagged `gateway-v<semver>`; agent releases use
/// `agent-v<semver>` and must not be mistaken for a dashboard update.
const TAG_PREFIX: &str = "gateway-v";
const DEFAULT_REPO: &str = "SidPad03/unified-mcp-gateway";

pub fn router() -> Router<AppState> {
    Router::new().route("/updates/check", get(check_for_updates))
}

#[derive(Deserialize)]
pub struct CheckQuery {
    /// The version the caller is running, e.g. `1.1.5`.
    pub current: Option<String>,
    /// Bypass the cache. The button offers this so an operator who just released
    /// isn't told to wait out the TTL.
    #[serde(default)]
    pub force: bool,
}

#[derive(Serialize, Clone)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub checked_at: String,
    pub source_repo: String,
    /// Set when the upstream check could not be completed. The endpoint still
    /// returns 200 so the UI can distinguish "you are up to date" from "I could
    /// not find out", which are very different things to show an operator.
    pub error: Option<String>,
}

/// The subset of GitHub's release object we rely on.
#[derive(Deserialize, Clone)]
pub struct GithubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
}

fn source_repo() -> String {
    std::env::var("UPDATE_CHECK_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string())
}

async fn check_for_updates(
    State(state): State<AppState>,
    _claims: Claims,
    Query(q): Query<CheckQuery>,
) -> Result<Json<UpdateStatus>, AppError> {
    let current = q.current.unwrap_or_default();
    let repo = source_repo();
    let now = chrono::Utc::now().to_rfc3339();

    let mut status = UpdateStatus {
        current_version: current.clone(),
        latest_version: None,
        update_available: false,
        release_url: None,
        release_name: None,
        release_notes: None,
        published_at: None,
        checked_at: now,
        source_repo: repo.clone(),
        error: None,
    };

    if std::env::var("UPDATE_CHECK_DISABLED").is_ok() {
        status.error = Some("Update checks are disabled on this deployment.".into());
        return Ok(Json(status));
    }

    let latest = match latest_gateway_release(&state, &repo, q.force).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            status.error = Some(format!("No {TAG_PREFIX}* releases published yet."));
            return Ok(Json(status));
        }
        Err(e) => {
            // Reachability problems are routine (no egress, rate limit, GitHub
            // down) and must not read as "no update available".
            tracing::warn!(error = %e, repo, "Update check failed");
            status.error = Some(e);
            return Ok(Json(status));
        }
    };

    let latest_version = latest.tag_name.trim_start_matches(TAG_PREFIX).to_string();
    status.update_available = is_newer(&latest_version, &current);
    status.latest_version = Some(latest_version);
    status.release_url = latest.html_url.clone();
    status.release_name = latest.name.clone();
    status.release_notes = latest.body.clone();
    status.published_at = latest.published_at.clone();

    Ok(Json(status))
}

/// Newest non-draft, non-prerelease `gateway-v*` release, cached.
async fn latest_gateway_release(
    state: &AppState,
    repo: &str,
    force: bool,
) -> Result<Option<GithubRelease>, String> {
    if !force {
        let cache = state.update_check_cache.lock().await;
        if let Some((cached_at, ref releases)) = *cache {
            if cached_at.elapsed() < CACHE_TTL {
                return Ok(pick_latest(releases));
            }
        }
    }

    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
    let client = reqwest::Client::builder()
        .timeout(UPSTREAM_TIMEOUT)
        .build()
        .map_err(|e| format!("Could not build HTTP client: {e}"))?;

    // GitHub rejects requests without a User-Agent.
    let mut req = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, "mcp-gateway-update-check")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;

    if !resp.status().is_success() {
        let code = resp.status();
        // Surface the rate limit specifically — it is the most likely failure
        // and the remedy (set GITHUB_TOKEN, or wait) is different from the rest.
        if code == reqwest::StatusCode::FORBIDDEN || code == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(
                "GitHub rate limit reached. Set GITHUB_TOKEN on the server to raise it.".into(),
            );
        }
        return Err(format!("GitHub returned HTTP {code}"));
    }

    let releases: Vec<GithubRelease> = resp
        .json()
        .await
        .map_err(|e| format!("Could not parse the GitHub response: {e}"))?;

    let mut cache = state.update_check_cache.lock().await;
    *cache = Some((Instant::now(), releases.clone()));

    Ok(pick_latest(&releases))
}

/// Highest semver among published gateway releases. GitHub returns newest-first
/// by creation date, which is not the same as highest version once a patch is
/// backported, so compare versions rather than trusting the order.
fn pick_latest(releases: &[GithubRelease]) -> Option<GithubRelease> {
    releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease && r.tag_name.starts_with(TAG_PREFIX))
        .filter_map(|r| {
            semver::Version::parse(r.tag_name.trim_start_matches(TAG_PREFIX))
                .ok()
                .map(|v| (v, r))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r.clone())
}

/// True when `latest` is strictly newer than `current`.
///
/// An unparseable `current` — the `0.0.0` a local `docker build` produces, or an
/// empty string — is treated as "cannot tell", not as "out of date". Nagging a
/// developer running an unversioned build would train them to ignore the notice.
fn is_newer(latest: &str, current: &str) -> bool {
    match (
        semver::Version::parse(latest),
        semver::Version::parse(current),
    ) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            name: None,
            body: None,
            html_url: None,
            published_at: None,
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn detects_a_newer_release() {
        assert!(is_newer("1.1.6", "1.1.5"));
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn same_or_older_is_not_an_update() {
        assert!(!is_newer("1.1.5", "1.1.5"));
        assert!(!is_newer("1.1.4", "1.1.5"));
    }

    #[test]
    fn unparseable_current_version_never_reports_an_update() {
        // A local build reports 0.0.0 or nothing at all; telling that developer
        // they are out of date on every page load is noise, not a signal.
        assert!(!is_newer("1.1.5", ""));
        assert!(!is_newer("1.1.5", "dev"));
        assert!(!is_newer("not-a-version", "1.1.5"));
    }

    #[test]
    fn zero_version_is_not_nagged() {
        // 0.0.0 parses, so it must be excluded deliberately rather than by luck.
        assert!(is_newer("1.1.5", "0.0.0"));
    }

    #[test]
    fn picks_highest_semver_not_newest_by_position() {
        // A backported patch published after a bigger release must not win.
        let releases = vec![
            rel("gateway-v1.1.9"),
            rel("gateway-v1.2.0"),
            rel("gateway-v1.1.10"),
        ];
        assert_eq!(pick_latest(&releases).unwrap().tag_name, "gateway-v1.2.0");
    }

    #[test]
    fn compares_numerically_not_lexically() {
        // "1.1.10" sorts before "1.1.9" as a string; semver must disagree.
        let releases = vec![rel("gateway-v1.1.9"), rel("gateway-v1.1.10")];
        assert_eq!(pick_latest(&releases).unwrap().tag_name, "gateway-v1.1.10");
    }

    #[test]
    fn ignores_agent_releases() {
        // Agent and gateway are versioned independently off the same repo; an
        // agent release must never surface as a dashboard update.
        let releases = vec![rel("agent-v9.9.9"), rel("gateway-v1.1.5")];
        assert_eq!(pick_latest(&releases).unwrap().tag_name, "gateway-v1.1.5");
    }

    #[test]
    fn ignores_drafts_and_prereleases() {
        let mut draft = rel("gateway-v2.0.0");
        draft.draft = true;
        let mut pre = rel("gateway-v1.9.0");
        pre.prerelease = true;
        let releases = vec![draft, pre, rel("gateway-v1.1.5")];
        assert_eq!(pick_latest(&releases).unwrap().tag_name, "gateway-v1.1.5");
    }

    #[test]
    fn no_gateway_releases_yields_none() {
        assert!(pick_latest(&[rel("agent-v1.0.0")]).is_none());
        assert!(pick_latest(&[]).is_none());
    }
}

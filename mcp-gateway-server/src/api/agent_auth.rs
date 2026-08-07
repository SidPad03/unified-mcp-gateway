//! Browser sign-in for the macOS agent.
//!
//! OAuth 2.0 authorization code with PKCE (RFC 7636), shaped for a native app
//! the way RFC 8252 recommends. The agent never asks anyone to paste an API key:
//!
//! ```text
//!   app                          browser                     gateway
//!    │  open /agent/authorize ──────►                            │
//!    │      ?code_challenge=S256(v)  │ ── GET ──────────────────► │  approval page
//!    │                               │ ◄─ sign in, "Authorize" ─► │
//!    │                               │ ── POST …/approve ───────► │  mints an mcpgw_ key
//!    │  ◄── redirect …?code=…&state=─┘                            │
//!    │  POST /agent/token {code, code_verifier} ────────────────► │  verifies the challenge
//!    │  ◄── { api_key, username, … } ─────────────────────────────┘
//! ```
//!
//! The credential the agent ends up holding is still an ordinary `mcpgw_` API
//! key, so the tunnel and its authentication are unchanged. What has gone away
//! is the human step of creating one in the dashboard and copying it into a
//! config file.
//!
//! Three deliberate choices:
//!
//! * **Any authenticated user may authorize an agent, but only for themselves.**
//!   `POST /api-keys` is owner-only because it can mint a key for *any* user;
//!   this endpoint always uses the caller's own identity, which is strictly
//!   narrower.
//! * **Pending authorizations live in memory**, not in Postgres. They expire in
//!   five minutes, and a gateway restart mid-sign-in is a retry rather than a
//!   support case. It does mean the flow needs the browser and the app to reach
//!   the same instance, which is worth knowing if you ever run replicas.
//! * **The approval page is served by the API**, not the dashboard, so sign-in
//!   works on a deployment where the dashboard is not installed or sits on
//!   another origin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::auth::Claims;
use crate::{AppError, AppState};

/// How long a sign-in has to complete, start to finish.
const REQUEST_TTL: Duration = Duration::from_secs(300);
/// Ceiling on concurrent sign-ins, so an unauthenticated caller hammering
/// `/agent/authorize` cannot grow the map without bound.
const MAX_PENDING: usize = 64;
/// The app's redirect. Registered as `CFBundleURLTypes` in the .app bundle and
/// intercepted by `ASWebAuthenticationSession`.
const APP_REDIRECT_URI: &str = "mcp-gateway-agent://auth/callback";

// ── Store ───────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AgentAuthStore {
    pending: Mutex<HashMap<String, Pending>>,
}

struct Pending {
    challenge: String,
    redirect_uri: String,
    client_state: String,
    agent_id: String,
    created: Instant,
    granted: Option<Grant>,
}

struct Grant {
    code: String,
    api_key: String,
    username: String,
    user_id: String,
    is_owner: bool,
}

impl AgentAuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    async fn insert(&self, id: String, pending: Pending) -> Result<(), AppError> {
        let mut map = self.pending.lock().await;
        map.retain(|_, entry| entry.created.elapsed() < REQUEST_TTL);
        if map.len() >= MAX_PENDING {
            return Err(AppError::BadRequest(
                "Too many sign-ins are already in progress. Try again in a few minutes.".into(),
            ));
        }
        map.insert(id, pending);
        Ok(())
    }
}

// ── Routes ──────────────────────────────────────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agent/authorize", get(authorize_page))
        .route("/agent/authorize/approve", post(approve))
        .route("/agent/token", post(exchange_token))
}

#[derive(Deserialize)]
pub struct AuthorizeQuery {
    /// What this machine wants to be called on the gateway.
    agent_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}

/// Serve the approval page.
async fn authorize_page(
    State(state): State<AppState>,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Response, AppError> {
    // PKCE only, and only the hashed variant. `plain` exists in the RFC for
    // clients that cannot compute SHA-256; this one can.
    if query.code_challenge_method != "S256" {
        return Err(AppError::BadRequest(
            "code_challenge_method must be S256".into(),
        ));
    }
    if !(43..=128).contains(&query.code_challenge.len()) {
        return Err(AppError::BadRequest("Malformed code_challenge".into()));
    }
    if query.state.is_empty() || query.state.len() > 128 {
        return Err(AppError::BadRequest("Malformed state".into()));
    }
    if !is_allowed_redirect(&query.redirect_uri) {
        return Err(AppError::BadRequest(format!(
            "redirect_uri must be {APP_REDIRECT_URI} or an http://127.0.0.1 loopback"
        )));
    }
    let agent_id = query.agent_id.trim();
    if agent_id.is_empty() || agent_id.len() > 64 {
        return Err(AppError::BadRequest("Malformed agent_id".into()));
    }

    let request_id = Uuid::new_v4().to_string();
    state
        .agent_auth
        .insert(
            request_id.clone(),
            Pending {
                challenge: query.code_challenge.clone(),
                redirect_uri: query.redirect_uri.clone(),
                client_state: query.state.clone(),
                agent_id: agent_id.to_string(),
                created: Instant::now(),
                granted: None,
            },
        )
        .await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    // Nothing about this page should ever be framed, cached, or referred on.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; \
             connect-src 'self'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );

    Ok((headers, approval_html(&request_id, agent_id)).into_response())
}

#[derive(Deserialize)]
pub struct ApproveRequest {
    request_id: String,
}

#[derive(Serialize)]
pub struct ApproveResponse {
    redirect_uri: String,
}

/// The user has signed in and pressed Authorize. Mint the key and hand back a
/// one-time code.
async fn approve(
    State(state): State<AppState>,
    claims: Claims,
    Json(request): Json<ApproveRequest>,
) -> Result<Json<ApproveResponse>, AppError> {
    let user_id: Uuid = claims
        .sub
        .parse()
        .map_err(|_| AppError::Internal("Invalid caller ID".into()))?;

    let mut map = state.agent_auth.pending.lock().await;
    map.retain(|_, entry| entry.created.elapsed() < REQUEST_TTL);

    let pending = map.get_mut(&request.request_id).ok_or_else(|| {
        AppError::BadRequest("This sign-in expired. Try again from the app.".into())
    })?;

    if pending.granted.is_some() {
        return Err(AppError::BadRequest(
            "This sign-in has already been approved.".into(),
        ));
    }

    // Mint an ordinary API key, owned by whoever just signed in.
    let raw_key = super::api_keys::generate_api_key();
    let key_prefix = raw_key[..12].to_string();
    let key_hash = super::api_keys::hash_key(&raw_key);
    let key_secret = super::api_keys::encrypt_api_key(&raw_key).ok();

    sqlx::query(
        "INSERT INTO api_keys (key_id, user_id, key_hash, key_prefix, name, key_secret)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(format!("agent: {}", pending.agent_id))
    .bind(&key_secret)
    .execute(&state.db)
    .await?;

    let code = Uuid::new_v4().to_string();
    let is_owner = claims.roles.iter().any(|role| role == "owner");
    let redirect_uri = append_query(
        &pending.redirect_uri,
        &[("code", &code), ("state", &pending.client_state)],
    );

    tracing::info!(
        user = %claims.username,
        agent_id = %pending.agent_id,
        "Authorized an agent"
    );

    pending.granted = Some(Grant {
        code,
        api_key: raw_key,
        username: claims.username.clone(),
        user_id: claims.sub.clone(),
        is_owner,
    });

    Ok(Json(ApproveResponse { redirect_uri }))
}

#[derive(Deserialize)]
pub struct TokenRequest {
    code: String,
    code_verifier: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub api_key: String,
    pub agent_id: String,
    pub username: String,
    pub user_id: String,
    /// Whether this account sees every user's calls or only its own — the app
    /// states which on the Audit and Usage pages (decision D4).
    pub is_owner: bool,
}

/// Exchange the one-time code for the key. Unauthenticated by design: the PKCE
/// verifier *is* the proof, and it is known only to the app instance that
/// started the flow.
async fn exchange_token(
    State(state): State<AppState>,
    Json(request): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let mut map = state.agent_auth.pending.lock().await;
    map.retain(|_, entry| entry.created.elapsed() < REQUEST_TTL);

    // A linear scan over at most MAX_PENDING entries, which is cheaper than
    // keeping a second index in step with this one.
    let request_id = map
        .iter()
        .find(|(_, entry)| {
            entry
                .granted
                .as_ref()
                .is_some_and(|grant| grant.code == request.code)
        })
        .map(|(id, _)| id.clone())
        .ok_or_else(|| AppError::Unauthorized("That code is not valid or has expired".into()))?;

    // Remove first, whatever happens next: a code is single-use, including when
    // the verifier turns out to be wrong.
    let pending = map
        .remove(&request_id)
        .ok_or_else(|| AppError::Internal("Authorization vanished".into()))?;

    if !verify_pkce(&request.code_verifier, &pending.challenge) {
        tracing::warn!("Agent token exchange failed the PKCE check");
        return Err(AppError::Unauthorized(
            "The code verifier does not match".into(),
        ));
    }

    let grant = pending
        .granted
        .ok_or_else(|| AppError::Internal("Authorization was not granted".into()))?;

    Ok(Json(TokenResponse {
        api_key: grant.api_key,
        agent_id: pending.agent_id,
        username: grant.username,
        user_id: grant.user_id,
        is_owner: grant.is_owner,
    }))
}

// ── PKCE ────────────────────────────────────────────────────────────────

/// `BASE64URL-ENCODE(SHA256(ASCII(verifier))) == challenge`, per RFC 7636 §4.6.
fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    if !(43..=128).contains(&verifier.len()) {
        return false;
    }
    let digest = Sha256::digest(verifier.as_bytes());
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    // Constant-time compare: the challenge is not secret, but there is no
    // reason to leak a match prefix either.
    constant_time_eq(computed.as_bytes(), challenge.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── Redirect handling ───────────────────────────────────────────────────

/// The app's custom scheme, or a loopback address.
///
/// RFC 8252 allows a native app any port on the loopback interface, which is
/// why the port is not pinned. Everything else is rejected — an open redirector
/// here would hand the code to whoever asked.
fn is_allowed_redirect(uri: &str) -> bool {
    if uri == APP_REDIRECT_URI {
        return true;
    }
    for prefix in ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"] {
        if let Some(rest) = uri.strip_prefix(prefix) {
            let (port, path) = match rest.split_once('/') {
                Some((port, path)) => (port, path),
                None => (rest, ""),
            };
            if !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit())
                && port.parse::<u16>().is_ok()
                && !path.contains('?')
                && !path.contains('#')
            {
                return true;
            }
        }
    }
    false
}

fn append_query(uri: &str, params: &[(&str, &str)]) -> String {
    let mut out = uri.to_string();
    for (index, (key, value)) in params.iter().enumerate() {
        out.push(if index == 0 && !uri.contains('?') {
            '?'
        } else {
            '&'
        });
        out.push_str(key);
        out.push('=');
        out.push_str(&urlencode(value));
    }
    out
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ── The page ────────────────────────────────────────────────────────────

fn approval_html(request_id: &str, agent_id: &str) -> String {
    // Styled with the dashboard's tokens so it does not read as a different
    // product mid-sign-in. Self-contained: no fonts, scripts or images fetched
    // from anywhere, which is also what lets the CSP above be as tight as it is.
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Authorize MCP Gateway Agent</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; min-height: 100vh; display: grid; place-items: center;
    background: #0a0a0f; color: #e5e7eb;
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, 'Segoe UI', Inter, sans-serif;
  }}
  .card {{
    width: min(420px, calc(100vw - 32px));
    background: #0f0f17; border: 1px solid #1e1e2e; border-radius: 16px;
    padding: 28px;
  }}
  .mark {{
    width: 40px; height: 40px; border-radius: 12px; display: grid; place-items: center;
    background: rgba(124, 92, 252, .16); margin-bottom: 18px;
  }}
  h1 {{ font-size: 17px; margin: 0 0 6px; letter-spacing: -.01em; }}
  p.sub {{ margin: 0 0 22px; color: #9ca3af; font-size: 13px; }}
  .agent {{
    display: flex; align-items: center; gap: 10px; margin-bottom: 22px;
    padding: 12px 14px; border: 1px solid #1e1e2e; border-radius: 10px; background: #0c0c14;
  }}
  .agent b {{ font-weight: 600; font-size: 13px; }}
  .agent span {{ color: #6b7280; font-size: 11px; text-transform: uppercase; letter-spacing: .12em; }}
  label {{ display: block; font-size: 11px; text-transform: uppercase; letter-spacing: .12em;
           color: #9ca3af; margin: 0 0 6px; }}
  input {{
    width: 100%; padding: 10px 12px; margin-bottom: 14px; font-size: 14px;
    color: #e5e7eb; background: #0c0c14; border: 1px solid #1e1e2e; border-radius: 10px;
  }}
  input:focus {{ outline: none; border-color: #7c5cfc; box-shadow: 0 0 0 3px rgba(124,92,252,.18); }}
  button {{
    width: 100%; padding: 11px 14px; font-size: 14px; font-weight: 600; cursor: pointer;
    color: #fff; background: #7c5cfc; border: 0; border-radius: 10px;
  }}
  button:hover:not(:disabled) {{ background: #6a4de0; }}
  button:disabled {{ opacity: .55; cursor: default; }}
  .msg {{ margin-top: 14px; font-size: 13px; min-height: 19px; }}
  .msg.error {{ color: #ef4444; }}
  .msg.ok {{ color: #22c55e; }}
  .done {{ text-align: center; }}
  .done svg {{ margin-bottom: 14px; }}
</style>
</head>
<body>
  <main class="card" id="card">
    <div class="mark" aria-hidden="true">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#7c5cfc"
           stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 5l5.5 5.5M3 12h5.5M3 19l5.5-5.5"/>
        <path d="M12 8l4 4-4 4-4-4z"/>
        <path d="M16 12h5"/>
      </svg>
    </div>
    <h1>Authorize MCP Gateway Agent</h1>
    <p class="sub">Sign in to let this Mac connect its local MCP servers to the gateway.</p>

    <div class="agent">
      <div>
        <span>Machine</span>
        <div><b>{agent}</b></div>
      </div>
    </div>

    <form id="form" autocomplete="on">
      <label for="username">Username</label>
      <input id="username" name="username" autocomplete="username" autofocus required>
      <label for="password">Password</label>
      <input id="password" name="password" type="password" autocomplete="current-password" required>
      <button type="submit" id="submit">Sign in and authorize</button>
    </form>
    <div class="msg" id="msg" role="status" aria-live="polite"></div>
  </main>

<script>
(function () {{
  var requestId = "{request_id}";
  // This page is served at <prefix>/agent/authorize. Deriving the API prefix
  // from the path rather than hard-coding /api/v1 keeps sign-in working behind
  // a reverse proxy that mounts the gateway somewhere else.
  var base = window.location.pathname.replace(/\/agent\/authorize\/?$/, '');
  var form = document.getElementById('form');
  var msg = document.getElementById('msg');
  var submit = document.getElementById('submit');

  function say(text, kind) {{
    msg.textContent = text;
    msg.className = 'msg' + (kind ? ' ' + kind : '');
  }}

  form.addEventListener('submit', function (event) {{
    event.preventDefault();
    submit.disabled = true;
    say('Signing in…');

    fetch(base + '/auth/login', {{
      method: 'POST',
      headers: {{ 'Content-Type': 'application/json' }},
      body: JSON.stringify({{
        username: document.getElementById('username').value,
        password: document.getElementById('password').value
      }})
    }})
      .then(function (response) {{
        if (!response.ok) throw new Error('Incorrect username or password.');
        return response.json();
      }})
      .then(function (auth) {{
        if (auth.user && auth.user.must_change_password) {{
          throw new Error('Set a new password in the dashboard first, then try again.');
        }}
        say('Authorizing…');
        return fetch(base + '/agent/authorize/approve', {{
          method: 'POST',
          headers: {{
            'Content-Type': 'application/json',
            'Authorization': 'Bearer ' + auth.token
          }},
          body: JSON.stringify({{ request_id: requestId }})
        }});
      }})
      .then(function (response) {{
        return response.json().then(function (body) {{
          if (!response.ok) throw new Error(body.error || 'Could not authorize this Mac.');
          return body;
        }});
      }})
      .then(function (body) {{
        say('Authorized. Returning to the app…', 'ok');
        document.getElementById('card').innerHTML =
          '<div class="done"><h1>You’re signed in</h1>' +
          '<p class="sub">You can close this window and go back to MCP Gateway Agent.</p></div>';
        window.location.href = body.redirect_uri;
      }})
      .catch(function (error) {{
        submit.disabled = false;
        say(error.message || 'Something went wrong.', 'error');
      }});
  }});
}})();
</script>
</body>
</html>"##,
        agent = escape_html(agent_id),
        request_id = escape_html(request_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_accepts_only_the_matching_verifier() {
        // RFC 7636 §4.6 worked example.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce(verifier, challenge));
        assert!(!verify_pkce(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXj",
            challenge
        ));
    }

    #[test]
    fn pkce_rejects_a_verifier_outside_the_legal_length() {
        assert!(!verify_pkce("too-short", "whatever"));
        assert!(!verify_pkce(&"a".repeat(129), "whatever"));
    }

    #[test]
    fn only_the_app_scheme_and_loopback_are_accepted_redirects() {
        assert!(is_allowed_redirect(APP_REDIRECT_URI));
        assert!(is_allowed_redirect("http://127.0.0.1:49152/callback"));
        assert!(is_allowed_redirect("http://localhost:1234/callback"));

        // An open redirector here would hand the code to an attacker.
        assert!(!is_allowed_redirect("https://evil.example.com/callback"));
        assert!(!is_allowed_redirect("http://127.0.0.1.evil.com/callback"));
        assert!(!is_allowed_redirect("mcp-gateway-agent://auth/other"));
        assert!(!is_allowed_redirect("http://127.0.0.1:notaport/callback"));
        assert!(!is_allowed_redirect("http://127.0.0.1:99999/callback"));
    }

    #[test]
    fn the_redirect_carries_the_code_and_the_state_back() {
        let uri = append_query(APP_REDIRECT_URI, &[("code", "abc"), ("state", "x y/z")]);
        assert_eq!(
            uri,
            "mcp-gateway-agent://auth/callback?code=abc&state=x%20y%2Fz"
        );
        // A redirect that already has a query keeps it.
        let loopback = append_query("http://127.0.0.1:9/cb?a=1", &[("code", "abc")]);
        assert_eq!(loopback, "http://127.0.0.1:9/cb?a=1&code=abc");
    }

    #[test]
    fn the_agent_name_cannot_inject_markup_into_the_page() {
        let html = approval_html(
            "11111111-1111-1111-1111-111111111111",
            "<img src=x onerror=alert(1)>",
        );
        assert!(!html.contains("<img src=x"), "agent_id was not escaped");
        assert!(html.contains("&lt;img src=x"));
    }

    #[test]
    fn the_page_calls_the_api_relative_to_its_own_prefix() {
        // The page lives at <prefix>/agent/authorize, so a bare relative
        // 'login' would resolve to <prefix>/agent/login and 404. Both calls go
        // through the derived base.
        let html = approval_html("11111111-1111-1111-1111-111111111111", "mac");
        assert!(html.contains(r"replace(/\/agent\/authorize\/?$/, '')"));
        assert!(html.contains("base + '/auth/login'"));
        assert!(html.contains("base + '/agent/authorize/approve'"));
    }

    #[tokio::test]
    async fn the_pending_map_cannot_grow_without_bound() {
        let store = AgentAuthStore::new();
        let make = || Pending {
            challenge: "c".repeat(43),
            redirect_uri: APP_REDIRECT_URI.into(),
            client_state: "s".into(),
            agent_id: "mac".into(),
            created: Instant::now(),
            granted: None,
        };
        for index in 0..MAX_PENDING {
            store.insert(format!("id-{index}"), make()).await.unwrap();
        }
        assert!(store.insert("one-too-many".into(), make()).await.is_err());
    }
}

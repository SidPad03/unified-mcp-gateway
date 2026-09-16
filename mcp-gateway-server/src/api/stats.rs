//! A small, flat summary of the gateway for dashboards that are not this one.
//!
//! Written for [Homepage](https://gethomepage.dev)'s `customapi` widget, and
//! just as usable by Homarr, Glance, Dashy or a shell script: `GET /api/v1/stats`
//! returns a dozen top-level numbers, so a widget maps a field by its name
//! rather than walking into `/metrics/summary`, which is shaped for the Metrics
//! page and is ten times the size.
//!
//! **Why it has its own credential.** A homelab dashboard keeps its API keys in
//! a YAML file, and the obvious key to paste there — an owner's `mcpgw_` key —
//! can call every tool behind the gateway and reconfigure the gateway itself.
//! The stats token can do exactly one thing, read this endpoint. It is not an
//! API key: `resolve_bearer` finds no row for it, so every other route answers
//! 401. The owner creates it on the Settings page, it is shown once, and only
//! its SHA-256 is stored.
//!
//! An owner's own session or API key reads the endpoint too, which is what the
//! Settings page uses for its preview. Anyone else is refused: these are
//! deployment-wide aggregates, the same reason `/metrics/summary` is
//! owner-only.

use axum::{
    extract::State,
    http::{header, HeaderMap},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use rand::Rng;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::auth::{require_admin, resolve_bearer, Claims};
use crate::{AppError, AppState};

/// The `settings` row the token lives in.
const STATS_TOKEN_SETTING: &str = "stats_token";

/// What every stats token starts with.
///
/// No separator after `stats`, deliberately: the audit redactor catches a bare
/// `mcpgw_` followed by twelve or more alphanumerics, and an underscore there
/// would end the match after five, so a token pasted into a tool argument would
/// go into the trail in the clear.
const STATS_TOKEN_PREFIX: &str = "mcpgw_stats";

/// How many characters of a token are kept to identify it on the Settings page.
const DISPLAYED_PREFIX_LEN: usize = 16;

#[derive(Serialize, Debug, PartialEq)]
pub struct StatsSummary {
    /// Tools a client can call right now: enabled, on an enabled backend. The
    /// figure the Backends page heads with as "Tools behind the gate".
    pub tools: i64,
    /// Every tool the backends published, disabled ones included. The gateway's
    /// own control tools are in neither figure.
    pub tools_registered: i64,
    pub backends: i64,
    pub backends_enabled: i64,
    /// Enabled and answering.
    pub backends_healthy: i64,
    /// Enabled and not answering — the Backends page's "Needs attention".
    /// An `idle` backend has simply not been started, and is not counted.
    pub backends_unhealthy: i64,
    /// Macs whose agent is connected right now.
    pub agents_connected: i64,
    /// Rows in the audit trail in the last 24 hours, whatever their outcome.
    pub calls_24h: i64,
    /// Calls that failed, including a tool that answered `isError`.
    pub errors_24h: i64,
    /// Calls a policy refused.
    pub denied_24h: i64,
    /// `errors_24h / calls_24h`, as a fraction in 0..=1 — Homepage's `percent`
    /// format expects a percentage, so multiply it (see docs/homepage.md).
    pub error_rate_24h: f64,
    /// Mean duration of the last 24 hours' calls, in milliseconds.
    pub avg_latency_ms_24h: f64,
    /// RFC 3339; `None` when the trail is empty.
    pub last_call_at: Option<String>,
    pub version: &'static str,
}

#[derive(Serialize)]
pub struct StatsTokenStatus {
    pub configured: bool,
    pub prefix: Option<String>,
    pub created_at: Option<String>,
    pub created_by: Option<String>,
    pub last_used_at: Option<String>,
}

#[derive(Serialize)]
pub struct CreatedStatsToken {
    /// Shown once. Only its hash is kept.
    pub token: String,
    pub prefix: String,
    pub created_at: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/stats", get(stats)).route(
        "/stats/token",
        get(token_status).post(create_token).delete(revoke_token),
    )
}

// ── The summary ─────────────────────────────────────────────────────────

async fn stats(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match authorize(&state, &headers).await {
        Ok(()) => match summary(&state.db).await {
            // A widget polls this every few seconds; nothing in between should
            // hand it yesterday's numbers.
            Ok(summary) => ([(header::CACHE_CONTROL, "no-store")], Json(summary)).into_response(),
            Err(e) => AppError::from(e).into_response(),
        },
        Err(e) => e.into_response(),
    }
}

/// A stats token, or an owner.
///
/// The token is checked first and only when the value looks like one, so a
/// JWT or API key never costs a settings read, and a stats token never reaches
/// `resolve_api_key` to be looked up as a key it is not.
async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing authorization header".into()))?
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("Invalid authorization format".into()))?
        .trim();

    if token.starts_with(STATS_TOKEN_PREFIX) && token_matches(&state.db, token).await? {
        note_token_used(&state.db, token);
        return Ok(());
    }

    let (claims, must_change_password) = resolve_bearer(token, state).await?;
    if must_change_password {
        return Err(AppError::Forbidden(
            "You must change your password before continuing".into(),
        ));
    }
    require_admin(&claims)
}

/// Every figure in one statement, so they describe the same instant.
///
/// Each has a twin elsewhere that it must agree with, and the filters are
/// copied from those rather than restated: `tools` is `backends::tool_counts`'
/// `enabled`, `backends_unhealthy` is the Backends page's "Needs attention",
/// and the 24-hour figures are `/metrics/summary` at `range=24h`.
pub async fn summary(db: &sqlx::PgPool) -> Result<StatsSummary, sqlx::Error> {
    let row: (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        Option<f64>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM tool_registry t JOIN backends b ON b.backend_id = t.backend_id \
             WHERE t.is_internal = FALSE AND t.is_enabled = TRUE AND b.is_enabled = TRUE), \
           (SELECT COUNT(*) FROM tool_registry t JOIN backends b ON b.backend_id = t.backend_id \
             WHERE t.is_internal = FALSE), \
           (SELECT COUNT(*) FROM backends), \
           (SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE), \
           (SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE AND health_status = 'healthy'), \
           (SELECT COUNT(*) FROM backends \
             WHERE is_enabled = TRUE AND health_status NOT IN ('healthy', 'idle')), \
           (SELECT COUNT(*) FROM backends \
             WHERE transport = 'agent' AND is_enabled = TRUE AND health_status = 'healthy'), \
           (SELECT COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours'), \
           (SELECT COUNT(*) FROM audit_events \
             WHERE status IN ('error', 'tool_error') AND timestamp > NOW() - INTERVAL '24 hours'), \
           (SELECT COUNT(*) FROM audit_events \
             WHERE status = 'denied' AND timestamp > NOW() - INTERVAL '24 hours'), \
           (SELECT AVG(duration_ms) FROM audit_events \
             WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'), \
           (SELECT MAX(timestamp) FROM audit_events)",
    )
    .fetch_one(db)
    .await?;

    let (
        tools,
        tools_registered,
        backends,
        backends_enabled,
        backends_healthy,
        backends_unhealthy,
        agents_connected,
        calls_24h,
        errors_24h,
        denied_24h,
        avg_latency,
        last_call,
    ) = row;

    Ok(StatsSummary {
        tools,
        tools_registered,
        backends,
        backends_enabled,
        backends_healthy,
        backends_unhealthy,
        agents_connected,
        calls_24h,
        errors_24h,
        denied_24h,
        error_rate_24h: if calls_24h > 0 {
            errors_24h as f64 / calls_24h as f64
        } else {
            0.0
        },
        avg_latency_ms_24h: avg_latency.unwrap_or(0.0),
        last_call_at: last_call.map(|t| t.to_rfc3339()),
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ── The token ───────────────────────────────────────────────────────────

async fn token_status(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<StatsTokenStatus>, AppError> {
    require_admin(&claims)?;

    let stored = read_token(&state.db).await?;
    let field = |key: &str| {
        stored
            .as_ref()
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    Ok(Json(StatsTokenStatus {
        configured: stored.is_some(),
        prefix: field("prefix"),
        created_at: field("created_at"),
        created_by: field("created_by"),
        last_used_at: field("last_used_at"),
    }))
}

/// Issue a token, replacing any existing one — which stops working at once.
async fn create_token(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<CreatedStatsToken>, AppError> {
    require_admin(&claims)?;

    let token = generate_token();
    let prefix: String = token.chars().take(DISPLAYED_PREFIX_LEN).collect();
    let created_at = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, NOW()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(STATS_TOKEN_SETTING)
    .bind(serde_json::json!({
        "hash": hash(&token),
        "prefix": prefix,
        "created_at": created_at,
        "created_by": claims.username,
    }))
    .execute(&state.db)
    .await?;

    tracing::info!(user = %claims.username, %prefix, "Stats token issued");

    Ok(Json(CreatedStatsToken {
        token,
        prefix,
        created_at,
    }))
}

async fn revoke_token(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let revoked = sqlx::query("DELETE FROM settings WHERE key = $1")
        .bind(STATS_TOKEN_SETTING)
        .execute(&state.db)
        .await?
        .rows_affected()
        > 0;
    if revoked {
        tracing::info!(user = %claims.username, "Stats token revoked");
    }
    Ok(Json(serde_json::json!({ "revoked": revoked })))
}

async fn read_token(db: &sqlx::PgPool) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = $1")
            .bind(STATS_TOKEN_SETTING)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(value,)| value))
}

async fn token_matches(db: &sqlx::PgPool, presented: &str) -> Result<bool, sqlx::Error> {
    let Some(stored) = read_token(db).await? else {
        return Ok(false);
    };
    let Some(expected) = stored.get("hash").and_then(|h| h.as_str()) else {
        return Ok(false);
    };
    Ok(constant_time_eq(
        hash(presented).as_bytes(),
        expected.as_bytes(),
    ))
}

/// Record when the token was last presented, at most once a minute.
///
/// A widget polls every few seconds, and a settings write per poll would be a
/// write per poll for a timestamp nobody reads to the second. The condition is
/// in the `UPDATE` itself, so there is no read to race — and so is the token's
/// hash, so a poll that lands just after **Regenerate** cannot mark the new
/// token as used by the widget still holding the old one.
fn note_token_used(db: &sqlx::PgPool, token: &str) {
    let db = db.clone();
    let hash = hash(token);
    tokio::spawn(async move {
        let _ = sqlx::query(
            "UPDATE settings \
             SET value = jsonb_set(value, '{last_used_at}', to_jsonb(to_char(NOW() AT TIME ZONE 'UTC', \
                 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'))) \
             WHERE key = $1 AND value->>'hash' = $2 AND (value->>'last_used_at' IS NULL \
                OR (value->>'last_used_at')::timestamptz < NOW() - INTERVAL '1 minute')",
        )
        .bind(STATS_TOKEN_SETTING)
        .bind(hash)
        .execute(&db)
        .await;
    });
}

fn generate_token() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random: String = (0..40)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect();
    format!("{STATS_TOKEN_PREFIX}{random}")
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Comparing digests, so a timing difference could only ever say how much of a
/// SHA-256 matched — which reveals nothing about the token. Constant time
/// anyway, because it costs nothing and nobody should have to make that
/// argument again.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_db;

    #[test]
    fn a_token_is_recognisable_and_long_enough() {
        let token = generate_token();
        assert!(token.starts_with(STATS_TOKEN_PREFIX));
        assert_eq!(token.len(), STATS_TOKEN_PREFIX.len() + 40);
        assert_ne!(generate_token(), token);
    }

    /// The whole reason the prefix has no separator.
    #[test]
    fn the_audit_redactor_catches_a_token() {
        let token = generate_token();
        let redacted =
            crate::audit::redactor::Redactor::new().redact(&format!(r#"{{"note":"{token}"}}"#));
        assert!(!redacted.contains(&token), "{redacted}");
    }

    #[test]
    fn digests_compare_equal_only_when_equal() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    async fn test_pool() -> Option<sqlx::PgPool> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("TEST_DATABASE_URL is set but unreachable");
        crate::db::run_migrations(&pool)
            .await
            .expect("migrations should apply");
        for table in ["audit_events", "tool_registry", "backends"] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM settings WHERE key = $1")
            .bind(STATS_TOKEN_SETTING)
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    async fn backend(
        pool: &sqlx::PgPool,
        name: &str,
        transport: &str,
        enabled: bool,
        health: &str,
    ) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO backends (backend_id, name, transport, config, is_enabled, health_status) \
             VALUES ($1, $2, $3, '{}'::jsonb, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(transport)
        .bind(enabled)
        .bind(health)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn tool(
        pool: &sqlx::PgPool,
        backend: uuid::Uuid,
        name: &str,
        enabled: bool,
        internal: bool,
    ) {
        sqlx::query(
            "INSERT INTO tool_registry (tool_id, tool_name, backend_id, original_name, is_enabled, is_internal) \
             VALUES ($1, $2, $3, $2, $4, $5)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(name)
        .bind(backend)
        .bind(enabled)
        .bind(internal)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn call(pool: &sqlx::PgPool, status: &str, hours_ago: i32, duration_ms: f64) {
        sqlx::query(
            "INSERT INTO audit_events (event_id, timestamp, trace_id, tool_name, backend_name, status, duration_ms) \
             VALUES ($1, NOW() - make_interval(hours => $2), $3, 'fs__read', 'fs', $4, $5)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(hours_ago)
        .bind(uuid::Uuid::now_v7())
        .bind(status)
        .bind(duration_ms)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Each figure against the rule it is documented to follow.
    #[tokio::test]
    async fn the_summary_counts_what_it_says_it_counts() {
        let _guard = lock_db().await;
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };

        let fs = backend(&pool, "fs", "stdio", true, "healthy").await;
        tool(&pool, fs, "fs__read", true, false).await;
        tool(&pool, fs, "fs__write", false, false).await; // disabled: registered only

        let off = backend(&pool, "off", "streamable-http", false, "idle").await;
        tool(&pool, off, "off__x", true, false).await; // enabled, but its backend is not

        let mac = backend(&pool, "mac", "agent", true, "healthy").await;
        tool(&pool, mac, "mac__notes", true, false).await;
        tool(&pool, mac, "agent_list_local_servers", true, true).await; // internal: in neither

        backend(&pool, "broken", "sse", true, "unhealthy").await;
        backend(&pool, "gone-mac", "agent", true, "disconnected").await;
        backend(&pool, "fresh", "stdio", true, "idle").await;

        call(&pool, "success", 1, 100.0).await;
        call(&pool, "success", 2, 300.0).await;
        call(&pool, "tool_error", 3, 200.0).await;
        call(&pool, "error", 4, 400.0).await;
        call(&pool, "denied", 5, 0.0).await;
        call(&pool, "success", 30, 9999.0).await; // outside the window

        let s = summary(&pool).await.unwrap();
        assert_eq!(s.tools, 2, "fs__read and mac__notes");
        assert_eq!(s.tools_registered, 4);
        assert_eq!(s.backends, 6);
        assert_eq!(s.backends_enabled, 5);
        assert_eq!(s.backends_healthy, 2);
        assert_eq!(
            s.backends_unhealthy, 2,
            "unhealthy and disconnected, not idle"
        );
        assert_eq!(s.agents_connected, 1);
        assert_eq!(s.calls_24h, 5);
        assert_eq!(s.errors_24h, 2);
        assert_eq!(s.denied_24h, 1);
        assert!((s.error_rate_24h - 0.4).abs() < 1e-9);
        assert!((s.avg_latency_ms_24h - 200.0).abs() < 1e-9);
        assert!(s.last_call_at.is_some());

        // The same noun as the Backends page's headline.
        let counts = crate::api::backends::tool_counts(&pool).await.unwrap();
        assert_eq!(s.tools, counts.values().map(|c| c.enabled).sum::<i64>());
        assert_eq!(
            s.tools_registered,
            counts.values().map(|c| c.registered).sum::<i64>()
        );
    }

    #[tokio::test]
    async fn an_empty_gateway_is_all_zeroes_not_an_error() {
        let _guard = lock_db().await;
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };
        let s = summary(&pool).await.unwrap();
        assert_eq!((s.tools, s.backends, s.calls_24h), (0, 0, 0));
        assert_eq!(s.error_rate_24h, 0.0);
        assert_eq!(s.last_call_at, None);
    }

    #[tokio::test]
    async fn only_the_current_token_matches() {
        let _guard = lock_db().await;
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };

        let token = generate_token();
        assert!(
            !token_matches(&pool, &token).await.unwrap(),
            "none issued yet"
        );

        sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2)")
            .bind(STATS_TOKEN_SETTING)
            .bind(serde_json::json!({ "hash": hash(&token), "prefix": "x" }))
            .execute(&pool)
            .await
            .unwrap();
        assert!(token_matches(&pool, &token).await.unwrap());
        assert!(!token_matches(&pool, &generate_token()).await.unwrap());

        // Presenting a token that is no longer current stamps nothing.
        note_token_used(&pool, &generate_token());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(read_token(&pool)
            .await
            .unwrap()
            .and_then(|v| v.get("last_used_at").cloned())
            .is_none());

        // Presenting the current one stamps last use.
        note_token_used(&pool, &token);
        let mut stamped = None;
        for _ in 0..100 {
            stamped = read_token(&pool)
                .await
                .unwrap()
                .and_then(|v| v.get("last_used_at").cloned());
            if stamped.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let stamped = stamped.expect("last_used_at is written");
        assert!(
            chrono::DateTime::parse_from_rfc3339(stamped.as_str().unwrap()).is_ok(),
            "{stamped}"
        );

        sqlx::query("DELETE FROM settings WHERE key = $1")
            .bind(STATS_TOKEN_SETTING)
            .execute(&pool)
            .await
            .unwrap();
        assert!(!token_matches(&pool, &token).await.unwrap(), "revoked");
    }
}

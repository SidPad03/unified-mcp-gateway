//! Gateway-wide settings — the handful of switches that belong to the
//! deployment rather than to the browser looking at it.
//!
//! Everything else the Settings page holds (the public MCP URL, the OpenAI
//! token for AI classification) lives in `localStorage`, because it is either a
//! per-operator preference or a credential the gateway deliberately never sees.
//! What is here changes what the *server* does, so it is in Postgres and it is
//! the same for everyone.

use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};

use super::auth::{require_admin, Claims};
use crate::{AppError, AppState};

/// Whether the gateway offers its own `gateway_*` tools over MCP.
///
/// Defaults to **on**: an operator who upgraded into 1.2.0 already has them,
/// and a patch release should not quietly withdraw tools that clients may have
/// started using. Turning it off withdraws the namespace immediately — it is
/// read per request, not cached — and every call to it is refused.
pub const GATEWAY_TOOLS_ENABLED: &str = "gateway_tools_enabled";

#[derive(Serialize)]
pub struct SettingsResponse {
    pub gateway_tools_enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub gateway_tools_enabled: Option<bool>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/settings", get(get_settings).patch(update_settings))
}

/// Read one boolean setting, falling back to `default` when it has never been
/// written or the row is not a boolean.
///
/// A settings read that fails must not take a tool call down with it, so a
/// database error resolves to the default and is logged rather than returned.
pub async fn read_bool(db: &sqlx::PgPool, key: &str, default: bool) -> bool {
    let row: Result<Option<(serde_json::Value,)>, _> =
        sqlx::query_as("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(db)
            .await;

    match row {
        Ok(Some((value,))) => value.as_bool().unwrap_or(default),
        Ok(None) => default,
        Err(e) => {
            tracing::warn!(setting = key, error = %e, "Could not read setting; using the default");
            default
        }
    }
}

async fn write_bool(db: &sqlx::PgPool, key: &str, value: bool) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, NOW()) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()",
    )
    .bind(key)
    .bind(serde_json::json!(value))
    .execute(db)
    .await?;
    Ok(())
}

async fn get_settings(
    State(state): State<AppState>,
    _claims: Claims,
) -> Result<Json<SettingsResponse>, AppError> {
    Ok(Json(SettingsResponse {
        gateway_tools_enabled: read_bool(&state.db, GATEWAY_TOOLS_ENABLED, true).await,
    }))
}

async fn update_settings(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    require_admin(&claims)?;

    if let Some(enabled) = req.gateway_tools_enabled {
        write_bool(&state.db, GATEWAY_TOOLS_ENABLED, enabled).await?;
        tracing::info!(
            user = %claims.username,
            enabled,
            "Gateway self-configuration tools switched"
        );
    }

    get_settings(State(state), claims).await
}

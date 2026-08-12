use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::auth::{require_admin, Claims};
use crate::{register_discovered_tools, AppError, AppState};

#[derive(Serialize)]
pub struct BackendResponse {
    pub backend_id: String,
    pub name: String,
    pub transport: String,
    pub config: serde_json::Value,
    pub risk_category: Option<String>,
    pub is_enabled: bool,
    pub health_status: String,
    pub last_health_check: Option<String>,
    pub created_at: String,
    pub tool_count: i64,
}

#[derive(Deserialize)]
pub struct CreateBackendRequest {
    pub name: String,
    pub transport: String,
    pub config: serde_json::Value,
    pub risk_category: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/backends", get(list_backends).post(create_backend))
        .route(
            "/backends/:id",
            delete(delete_backend).patch(update_backend),
        )
        .route("/backends/:id/sync", post(sync_backend))
}

/// Stands in for a value the caller is not allowed to see.
///
/// A masked variable is stored in plain text like any other — masking is a rule
/// about what may be *displayed*. This placeholder is what the dashboard gets
/// instead, in the config panel, in the edit form and in the JSON editor alike,
/// and it is what the dashboard sends back for a variable the user did not
/// retype. [`restore_masked_values`] turns it back into the stored value before
/// anything is written or started, so the round trip is lossless.
///
/// The same string is spelled out in `mcp-gateway-agent-core`'s `config::MASKED`
/// — two crates, one contract; change both together.
const MASKED: &str = "__mcpgw_masked__";

/// Strip secret-bearing fields (env vars, auth headers) from a backend config
/// so they aren't exposed to non-admin callers. Admins keep the full config
/// because they manage backends; everyone else only needs names/transport.
fn redact_backend_config(mut config: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = config.as_object_mut() {
        obj.remove("env");
        obj.remove("headers");
    }
    config
}

/// Replace the values the user marked secret with [`MASKED`].
///
/// This runs for admins too. "Masked" would mean very little if the person who
/// set the flag could read the value back on the next page load, so the only way
/// to a masked value is to clear its flag in the editor and save.
fn mask_secret_values(mut config: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = config.as_object_mut() {
        mask_group(obj, "env", "masked_env");
        mask_group(obj, "headers", "masked_headers");
    }
    config
}

fn masked_keys(config: &serde_json::Map<String, serde_json::Value>, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(|v| v.as_array())
        .map(|keys| {
            keys.iter()
                .filter_map(|k| k.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn mask_group(
    config: &mut serde_json::Map<String, serde_json::Value>,
    values_key: &str,
    masked_key: &str,
) {
    let masked = masked_keys(config, masked_key);
    if masked.is_empty() {
        return;
    }
    if let Some(values) = config.get_mut(values_key).and_then(|v| v.as_object_mut()) {
        for key in masked {
            if let Some(slot) = values.get_mut(&key) {
                *slot = serde_json::Value::String(MASKED.into());
            }
        }
    }
}

/// Put the stored values back wherever the caller sent [`MASKED`].
///
/// `current` is the configuration already in the database. A placeholder with
/// nothing behind it — a new backend, or a key that did not exist before —
/// collapses to an empty string rather than being stored literally.
fn restore_masked_values(
    mut incoming: serde_json::Value,
    current: &serde_json::Value,
) -> serde_json::Value {
    if let Some(obj) = incoming.as_object_mut() {
        restore_group(obj, current, "env");
        restore_group(obj, current, "headers");
        tidy_masks(obj, "env", "masked_env");
        tidy_masks(obj, "headers", "masked_headers");
    }
    incoming
}

fn restore_group(
    incoming: &mut serde_json::Map<String, serde_json::Value>,
    current: &serde_json::Value,
    values_key: &str,
) {
    let stored = current.get(values_key).and_then(|v| v.as_object());
    let Some(values) = incoming.get_mut(values_key).and_then(|v| v.as_object_mut()) else {
        return;
    };
    for (key, slot) in values.iter_mut() {
        if slot.as_str() == Some(MASKED) {
            *slot = stored
                .and_then(|s| s.get(key))
                .cloned()
                .unwrap_or_else(|| serde_json::Value::String(String::new()));
        }
    }
}

/// Drop mask flags for keys that are no longer there, so a deleted variable
/// cannot leave a flag behind that masks a future variable of the same name.
fn tidy_masks(
    config: &mut serde_json::Map<String, serde_json::Value>,
    values_key: &str,
    masked_key: &str,
) {
    if !config.contains_key(masked_key) {
        return;
    }
    let present: Vec<String> = config
        .get(values_key)
        .and_then(|v| v.as_object())
        .map(|values| values.keys().cloned().collect())
        .unwrap_or_default();
    let mut kept: Vec<String> = masked_keys(config, masked_key)
        .into_iter()
        .filter(|k| present.contains(k))
        .collect();
    kept.sort();
    kept.dedup();
    if kept.is_empty() {
        config.remove(masked_key);
    } else {
        config.insert(masked_key.into(), serde_json::json!(kept));
    }
}

async fn list_backends(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<BackendResponse>>, AppError> {
    let is_admin = claims.roles.iter().any(|r| r == "owner");

    let backends: Vec<(Uuid, String, String, serde_json::Value, Option<String>, bool, String, Option<chrono::DateTime<chrono::Utc>>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT backend_id, name, transport, config, risk_category, is_enabled, health_status, last_health_check, created_at FROM backends ORDER BY name"
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::new();
    for (
        backend_id,
        name,
        transport,
        config,
        risk_category,
        is_enabled,
        health_status,
        last_health_check,
        created_at,
    ) in backends
    {
        let (tool_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1")
                .bind(backend_id)
                .fetch_one(&state.db)
                .await?;

        let config = if is_admin {
            mask_secret_values(config)
        } else {
            redact_backend_config(config)
        };

        result.push(BackendResponse {
            backend_id: backend_id.to_string(),
            name,
            transport,
            config,
            risk_category,
            is_enabled,
            health_status,
            last_health_check: last_health_check.map(|t| t.to_rfc3339()),
            created_at: created_at.to_rfc3339(),
            tool_count,
        });
    }

    Ok(Json(result))
}

async fn create_backend(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreateBackendRequest>,
) -> Result<Json<BackendResponse>, AppError> {
    require_admin(&claims)?;

    if !["stdio", "streamable-http", "sse", "agent"].contains(&req.transport.as_str()) {
        return Err(AppError::BadRequest(
            "Transport must be 'stdio', 'streamable-http', 'sse', or 'agent'".into(),
        ));
    }

    // Nothing to restore a placeholder from on a brand new backend, but the
    // JSON editor can carry one over from a config it was shown, and storing it
    // literally would hand the process a nonsense environment.
    let req = CreateBackendRequest {
        config: restore_masked_values(req.config, &serde_json::Value::Null),
        ..req
    };

    let backend_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backends (backend_id, name, transport, config, risk_category, is_enabled, health_status)
         VALUES ($1, $2, $3, $4, $5, TRUE, 'idle')"
    )
    .bind(backend_id)
    .bind(&req.name)
    .bind(&req.transport)
    .bind(&req.config)
    .bind(&req.risk_category)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate") {
            AppError::Conflict("Backend name already exists".into())
        } else {
            AppError::Internal(e.to_string())
        }
    })?;

    let mut health_status = "idle".to_string();
    let mut tool_count: i64 = 0;

    let discover_result = match req.transport.as_str() {
        "stdio" => Some(
            state
                .backend_manager
                .spawn_backend(backend_id, &req.name, &req.config)
                .await,
        ),
        "streamable-http" => {
            Some(crate::backends::BackendManager::discover_http_tools(&req.name, &req.config).await)
        }
        "sse" => {
            Some(crate::backends::BackendManager::discover_sse_tools(&req.name, &req.config).await)
        }
        _ => None,
    };

    if let Some(result) = discover_result {
        match result {
            Ok(tools) => {
                tool_count = tools.len() as i64;
                register_discovered_tools(&state.db, backend_id, &req.name, &tools).await;
                let _ = sqlx::query("UPDATE backends SET health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $1")
                    .bind(backend_id).execute(&state.db).await;
                health_status = "healthy".into();
                tracing::info!(backend = %req.name, transport = %req.transport, tools = tools.len(), "Backend created and started");
            }
            Err(e) => {
                let _ = sqlx::query("UPDATE backends SET health_status = 'unhealthy', last_health_check = NOW() WHERE backend_id = $1")
                    .bind(backend_id).execute(&state.db).await;
                health_status = "unhealthy".into();
                tracing::error!(backend = %req.name, error = %e, "Backend created but failed to start");
            }
        }
    }

    Ok(Json(BackendResponse {
        backend_id: backend_id.to_string(),
        name: req.name,
        transport: req.transport,
        config: mask_secret_values(req.config),
        risk_category: req.risk_category,
        is_enabled: true,
        health_status,
        last_health_check: Some(chrono::Utc::now().to_rfc3339()),
        created_at: chrono::Utc::now().to_rfc3339(),
        tool_count,
    }))
}

#[derive(Deserialize)]
pub struct UpdateBackendRequest {
    pub is_enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
    pub risk_category: Option<String>,
}

async fn update_backend(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateBackendRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    // Fetch current backend info for lifecycle management
    let row: Option<(String, String, serde_json::Value)> =
        sqlx::query_as("SELECT name, transport, config FROM backends WHERE backend_id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let (name, transport, current_config) = match row {
        Some(r) => r,
        None => return Err(AppError::NotFound("Backend not found".into())),
    };

    // The dashboard never held the masked values, so it sends placeholders back
    // for the ones the user did not retype. Resolve them once, here, and every
    // path below — the write, the respawn, the tool discovery — sees the real
    // configuration.
    let req = UpdateBackendRequest {
        config: req
            .config
            .map(|config| restore_masked_values(config, &current_config)),
        ..req
    };

    if let Some(is_enabled) = req.is_enabled {
        sqlx::query("UPDATE backends SET is_enabled = $1 WHERE backend_id = $2")
            .bind(is_enabled)
            .bind(id)
            .execute(&state.db)
            .await?;

        if is_enabled {
            let config = req.config.as_ref().unwrap_or(&current_config);
            let result = match transport.as_str() {
                "stdio" => Some(state.backend_manager.spawn_backend(id, &name, config).await),
                "streamable-http" => {
                    Some(crate::backends::BackendManager::discover_http_tools(&name, config).await)
                }
                "sse" => {
                    Some(crate::backends::BackendManager::discover_sse_tools(&name, config).await)
                }
                _ => None,
            };
            if let Some(result) = result {
                match result {
                    Ok(tools) => {
                        register_discovered_tools(&state.db, id, &name, &tools).await;
                        let _ = sqlx::query("UPDATE backends SET health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $1")
                            .bind(id).execute(&state.db).await;
                    }
                    Err(e) => {
                        let _ = sqlx::query("UPDATE backends SET health_status = 'unhealthy', last_health_check = NOW() WHERE backend_id = $1")
                            .bind(id).execute(&state.db).await;
                        tracing::error!(backend = %name, error = %e, "Failed to start backend");
                    }
                }
            }
        } else {
            if transport == "stdio" {
                state.backend_manager.stop_backend(&id).await;
            }
            let _ =
                sqlx::query("UPDATE tool_registry SET is_enabled = FALSE WHERE backend_id = $1")
                    .bind(id)
                    .execute(&state.db)
                    .await;
            let _ = sqlx::query("UPDATE backends SET health_status = 'idle', last_health_check = NOW() WHERE backend_id = $1")
                .bind(id).execute(&state.db).await;
        }
    }
    if let Some(config) = &req.config {
        sqlx::query("UPDATE backends SET config = $1 WHERE backend_id = $2")
            .bind(config)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(risk_category) = &req.risk_category {
        sqlx::query("UPDATE backends SET risk_category = $1 WHERE backend_id = $2")
            .bind(risk_category)
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

async fn delete_backend(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    // Stop the process before deleting from DB
    state.backend_manager.stop_backend(&id).await;

    // Remove discovered tools
    sqlx::query("DELETE FROM tool_registry WHERE backend_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    sqlx::query("DELETE FROM backends WHERE backend_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

async fn sync_backend(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let row: Option<(String, String, serde_json::Value, bool)> = sqlx::query_as(
        "SELECT name, transport, config, is_enabled FROM backends WHERE backend_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let (name, transport, config, is_enabled) = match row {
        Some(r) => r,
        None => return Err(AppError::NotFound("Backend not found".into())),
    };

    if !is_enabled {
        return Err(AppError::BadRequest(
            "Cannot sync a disabled backend".into(),
        ));
    }

    let result = match transport.as_str() {
        "stdio" => {
            state.backend_manager.stop_backend(&id).await;
            state
                .backend_manager
                .spawn_backend(id, &name, &config)
                .await
        }
        "streamable-http" => {
            crate::backends::BackendManager::discover_http_tools(&name, &config).await
        }
        "sse" => crate::backends::BackendManager::discover_sse_tools(&name, &config).await,
        "agent" => {
            // Extract agent_id from the backend config
            let agent_id = config
                .get("agent_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AppError::Internal("Agent backend missing agent_id in config".into())
                })?
                .to_string();

            // Send a resync request to the connected agent
            match state.agent_registry.request_resync(&agent_id).await {
                Ok(()) => {
                    // The agent will re-send its register message which updates tools in DB
                    // Give the agent a moment to respond, then return current tool count
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let (tool_count,): (i64,) =
                        sqlx::query_as("SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1")
                            .bind(id)
                            .fetch_one(&state.db)
                            .await?;

                    return Ok(Json(serde_json::json!({
                        "status": "synced",
                        "tools_discovered": tool_count,
                    })));
                }
                Err(e) => {
                    let _ = sqlx::query(
                        "UPDATE backends SET health_status = 'disconnected', last_health_check = NOW() WHERE backend_id = $1"
                    ).bind(id).execute(&state.db).await;

                    return Err(AppError::BadRequest(format!(
                        "Agent is not connected: {}. The agent will re-sync automatically when it reconnects.",
                        e
                    )));
                }
            }
        }
        _ => {
            return Err(AppError::BadRequest(format!(
                "Unsupported transport: {}",
                transport
            )))
        }
    };

    match result {
        Ok(tools) => {
            let tool_count = tools.len();
            register_discovered_tools(&state.db, id, &name, &tools).await;
            let _ = sqlx::query("UPDATE backends SET health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $1")
                .bind(id).execute(&state.db).await;

            Ok(Json(serde_json::json!({
                "status": "synced",
                "tools_discovered": tool_count,
            })))
        }
        Err(e) => {
            let _ = sqlx::query("UPDATE backends SET health_status = 'unhealthy', last_health_check = NOW() WHERE backend_id = $1")
                .bind(id).execute(&state.db).await;
            Err(AppError::Internal(format!("Sync failed: {}", e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stored() -> serde_json::Value {
        json!({
            "command": "gitea-mcp",
            "env": {"GITEA_TOKEN": "the-real-token", "GITEA_URL": "http://gitea.local"},
            "masked_env": ["GITEA_TOKEN"],
        })
    }

    #[test]
    fn a_masked_value_is_never_sent_to_the_browser() {
        let masked = mask_secret_values(stored());
        assert_eq!(masked["env"]["GITEA_TOKEN"], MASKED);
        assert_eq!(masked["env"]["GITEA_URL"], "http://gitea.local");
        assert!(
            !serde_json::to_string(&masked)
                .unwrap()
                .contains("the-real-token"),
            "the value leaked: {masked}"
        );
    }

    #[test]
    fn an_edit_that_does_not_retype_a_secret_keeps_it() {
        // The round trip the edit form and the JSON editor both make: what was
        // handed out comes back unchanged, and must land as it started.
        let round_tripped = restore_masked_values(mask_secret_values(stored()), &stored());
        assert_eq!(round_tripped, stored());
    }

    #[test]
    fn unmasking_a_variable_brings_its_value_back() {
        let mut edited = mask_secret_values(stored());
        edited["masked_env"] = json!([]);
        let saved = restore_masked_values(edited, &stored());

        assert_eq!(saved["env"]["GITEA_TOKEN"], "the-real-token");
        assert!(
            saved.get("masked_env").is_none(),
            "an empty mask list is dropped rather than stored: {saved}"
        );
        // And from here on the dashboard sees it.
        assert_eq!(
            mask_secret_values(saved)["env"]["GITEA_TOKEN"],
            "the-real-token"
        );
    }

    #[test]
    fn retyping_a_masked_value_replaces_it() {
        let mut edited = mask_secret_values(stored());
        edited["env"]["GITEA_TOKEN"] = json!("a-brand-new-token");
        let saved = restore_masked_values(edited, &stored());
        assert_eq!(saved["env"]["GITEA_TOKEN"], "a-brand-new-token");
        assert_eq!(saved["masked_env"], json!(["GITEA_TOKEN"]));
    }

    #[test]
    fn a_placeholder_with_nothing_behind_it_does_not_become_the_value() {
        let created = restore_masked_values(
            json!({"env": {"TOKEN": MASKED}, "masked_env": ["TOKEN"]}),
            &serde_json::Value::Null,
        );
        assert_eq!(created["env"]["TOKEN"], "");
    }

    #[test]
    fn a_removed_variable_takes_its_mask_with_it() {
        let saved = restore_masked_values(
            json!({"env": {"GITEA_URL": "http://gitea.local"}, "masked_env": ["GITEA_TOKEN"]}),
            &stored(),
        );
        assert!(saved.get("masked_env").is_none(), "{saved}");
    }

    #[test]
    fn masking_headers_works_the_same_way() {
        let config = json!({
            "url": "http://127.0.0.1:3010/mcp",
            "headers": {"Authorization": "Bearer sk-live-1234", "X-Trace": "on"},
            "masked_headers": ["Authorization"],
        });
        let masked = mask_secret_values(config.clone());
        assert_eq!(masked["headers"]["Authorization"], MASKED);
        assert_eq!(masked["headers"]["X-Trace"], "on");
        assert_eq!(restore_masked_values(masked, &config), config);
    }

    #[test]
    fn a_non_admin_still_gets_no_env_block_at_all() {
        let redacted = redact_backend_config(stored());
        assert!(redacted.get("env").is_none());
        assert_eq!(redacted["command"], "gitea-mcp");
    }
}

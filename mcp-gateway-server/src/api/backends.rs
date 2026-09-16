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
    /// Tools this backend published, excluding the gateway's own control tools.
    /// See [`tool_counts`] — this is the one definition of the noun.
    pub tool_count: i64,
    /// The subset a call can still reach: `tool_count` minus the tools the
    /// operator disabled, and zero when the backend itself is disabled. What
    /// "behind the gate" means, and what `tools/list` will serve.
    pub enabled_tool_count: i64,
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

/// The names a backend may not take.
///
/// `gateway` is what the audit trail files the gateway's own tools under
/// (`api::mcp::Target::backend_name`), and migration 012 deletes every row
/// filed under it. A backend allowed to take the name would have its history
/// deleted along with them and would be indistinguishable from the gateway in
/// every `backend=` filter.
const RESERVED_BACKEND_NAMES: &[&str] = &["gateway"];

/// Reject a backend name that cannot work before it reaches the unique index.
///
/// Applied on every write path — REST create and `gateway_register_backend` —
/// because a name checked in one place and not the other is how `gateway`
/// became registrable while `api::mcp` documented that it was not.
pub(crate) fn validate_backend_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("A backend needs a name".into());
    }
    if trimmed.len() > 255 {
        return Err("A backend name may be at most 255 characters".into());
    }
    if trimmed.contains("__") {
        return Err(
            "A backend name cannot contain '__' — that is the tool namespace separator".into(),
        );
    }
    if RESERVED_BACKEND_NAMES
        .iter()
        .any(|r| r.eq_ignore_ascii_case(trimmed))
    {
        return Err(format!(
            "'{trimmed}' is reserved — the gateway files its own tool calls under that name"
        ));
    }
    Ok(())
}

/// Environment variables the gateway reads for itself, and which therefore must
/// not reach a backend.
///
/// A stdio backend is third-party code the operator pointed at; the child
/// inherits this process's environment, so without this list an `npx`-fetched
/// server could read `JWT_SECRET` and mint an owner token, or read
/// `DATABASE_URL` and go straight to Postgres past auth, policy and the audit
/// trail. `the_gateways_own_secrets_are_stripped_from_a_backend` keeps the list
/// in step with what the server actually reads.
pub(crate) const GATEWAY_ONLY_ENV: &[&str] = &[
    "JWT_SECRET",
    "DATABASE_URL",
    "MCPGW_ADMIN_PASSWORD",
    "GITHUB_TOKEN",
    "TEST_DATABASE_URL",
];

/// Strip secret-bearing fields from a backend config so they aren't exposed to
/// non-admin callers. Admins keep the full config because they manage backends;
/// everyone else only needs the transport and the mask flags.
///
/// `command`, `args` and `url` go with `env` and `headers`: a stdio argv
/// routinely carries a credential — the dashboard's own Connect flow writes
/// `["-y", "mcp-remote", url, "--header", "Authorization: Bearer …"]` — and an
/// SSE URL can carry a session token in its query string. Removing only `env`
/// and `headers` left both readable by any authenticated account on the page a
/// non-owner lands on.
pub(crate) fn redact_backend_config(mut config: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = config.as_object_mut() {
        obj.remove("env");
        obj.remove("headers");
        obj.remove("command");
        obj.remove("args");
        obj.remove("url");
    }
    config
}

/// Replace the values the user marked secret with [`MASKED`].
///
/// This runs for admins too. "Masked" would mean very little if the person who
/// set the flag could read the value back on the next page load, so the only way
/// to a masked value is to clear its flag in the editor and save.
pub(crate) fn mask_secret_values(mut config: serde_json::Value) -> serde_json::Value {
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
pub(crate) fn restore_masked_values(
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

/// How many tools a backend has, for every backend, in one query.
///
/// **This is the only definition of "tools on this backend".** The noun is
/// drawn on the Backends page, on Metrics → Backend health, on the Usage
/// graph's backend nodes and by `gateway_list_backends`, and it was written out
/// by hand at each of them with a different filter: one excluded internal
/// tools, one excluded nothing, one excluded disabled tools instead. A
/// connected Mac therefore read 6, 15 and 15 on three pages of the same
/// gateway. Every caller now takes the pair from here.
///
/// `registered` is what the backend published and the gateway kept, minus the
/// gateway's own control tools — the figure CHANGELOG 1.2.1 promised. `enabled`
/// is the subset a `tools/call` can still reach, which is what "behind the
/// gate" means and is always the smaller of the two.
///
/// One grouped query rather than a `COUNT(*)` per row: the old loop issued one
/// round trip per backend on every page load, and this page polls.
pub(crate) async fn tool_counts(
    db: &sqlx::PgPool,
) -> Result<std::collections::HashMap<Uuid, ToolCounts>, sqlx::Error> {
    let rows: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT b.backend_id, \
                COUNT(t.tool_id) FILTER (WHERE t.is_internal = FALSE), \
                COUNT(t.tool_id) FILTER (WHERE t.is_internal = FALSE AND t.is_enabled = TRUE AND b.is_enabled = TRUE) \
         FROM backends b \
         LEFT JOIN tool_registry t ON t.backend_id = b.backend_id \
         GROUP BY b.backend_id",
    )
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(backend_id, registered, enabled)| {
            (
                backend_id,
                ToolCounts {
                    registered,
                    enabled,
                },
            )
        })
        .collect())
}

/// The two figures [`tool_counts`] returns. `enabled <= registered`, always.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolCounts {
    pub registered: i64,
    pub enabled: i64,
}

/// Bring a backend up and register whatever it advertises.
///
/// `Ok(None)` means there was nothing here to start: an `agent` backend runs on
/// somebody's Mac and registers itself when it dials in, so its health is not
/// this side's to set. `Ok(Some(n))` is a successful start with `n` tools
/// discovered.
///
/// Both the REST layer and the `gateway_*` tool namespace go through here, and
/// that is the point — an agent starting a backend has to land in exactly the
/// state the dashboard would have produced, health row and all.
pub(crate) async fn start_and_register(
    state: &AppState,
    backend_id: Uuid,
    name: &str,
    transport: &str,
    config: &serde_json::Value,
) -> Result<Option<usize>, String> {
    let discovered = match transport {
        "stdio" => {
            state
                .backend_manager
                .spawn_backend(backend_id, name, config)
                .await
        }
        "streamable-http" => {
            state
                .backend_manager
                .discover_http_tools(backend_id, name, config)
                .await
        }
        "sse" => crate::backends::BackendManager::discover_sse_tools(name, config).await,
        "agent" => return Ok(None),
        other => Err(format!("Unsupported transport: {other}")),
    };

    match discovered {
        Ok(tools) => {
            let count = tools.len();
            register_discovered_tools(&state.db, backend_id, name, &tools).await;
            let _ = sqlx::query(
                "UPDATE backends SET health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $1",
            )
            .bind(backend_id)
            .execute(&state.db)
            .await;
            tracing::info!(backend = %name, transport = %transport, tools = count, "Backend started");
            Ok(Some(count))
        }
        Err(e) => {
            let _ = sqlx::query(
                "UPDATE backends SET health_status = 'unhealthy', last_health_check = NOW() WHERE backend_id = $1",
            )
            .bind(backend_id)
            .execute(&state.db)
            .await;
            // Kept on the manager as well as in the log, so
            // `gateway_get_mcp_server_status` can say what went wrong rather
            // than only that something did.
            state.backend_manager.note_error(backend_id, &e).await;
            tracing::error!(backend = %name, transport = %transport, error = %e, "Failed to start backend");
            Err(e)
        }
    }
}

/// Take a backend down: stop its process, withdraw its tools, mark it idle.
///
/// The tools are disabled rather than deleted, so re-enabling the backend does
/// not lose a hand-set risk classification.
pub(crate) async fn stop_and_withdraw(state: &AppState, backend_id: Uuid) {
    // Every transport: a stdio backend has a process to stop, and a stateful
    // streamable-http one has a session to end.
    state.backend_manager.stop_backend(&backend_id).await;
    let _ = sqlx::query("UPDATE tool_registry SET is_enabled = FALSE WHERE backend_id = $1")
        .bind(backend_id)
        .execute(&state.db)
        .await;
    let _ = sqlx::query(
        "UPDATE backends SET health_status = 'idle', last_health_check = NOW() WHERE backend_id = $1",
    )
    .bind(backend_id)
    .execute(&state.db)
    .await;
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

    let counts = tool_counts(&state.db).await?;

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
        let count = counts.get(&backend_id).copied().unwrap_or_default();
        let tool_count = count.registered;
        let enabled_tool_count = count.enabled;

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
            enabled_tool_count,
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

    validate_backend_name(&req.name).map_err(AppError::BadRequest)?;

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

    let (health_status, tool_count) = match start_and_register(
        &state,
        backend_id,
        &req.name,
        &req.transport,
        &req.config,
    )
    .await
    {
        // An agent backend is registered by the Mac that runs it, so a row
        // created here just waits, idle, for that machine to dial in.
        Ok(None) => ("idle".to_string(), 0i64),
        Ok(Some(count)) => ("healthy".to_string(), count as i64),
        Err(_) => ("unhealthy".to_string(), 0i64),
    };

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
        // A backend is created enabled, and discovery registers every tool
        // enabled, so the two are equal for exactly as long as this response
        // takes to reach the client.
        tool_count,
        enabled_tool_count: tool_count,
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

    // An agent backend's configuration is not ours to write. The Mac running
    // the agent owns its command, environment and tool list, and it re-sends
    // all of it on every connection — `register_agent_in_db` upserts `config`
    // wholesale — so a write accepted here would be reverted without a word the
    // next time that agent reconnected. Refuse it rather than pretend.
    // Enabling and disabling still belong to the gateway, so those pass.
    if transport == "agent" && req.config.is_some() {
        return Err(AppError::BadRequest(
            "An agent backend is configured in the macOS app on the machine that runs it. The gateway can enable or disable it, but its configuration is not editable here."
                .into(),
        ));
    }

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
            let _ = start_and_register(&state, id, &name, &transport, config).await;
        } else {
            stop_and_withdraw(&state, id).await;
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
        "stdio" | "streamable-http" | "sse" => {
            start_and_register(&state, id, &name, &transport, &config).await
        }
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
                        sqlx::query_as(
                "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1 AND is_internal = FALSE",
            )
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
        Ok(tools_discovered) => Ok(Json(serde_json::json!({
            "status": "synced",
            "tools_discovered": tools_discovered.unwrap_or(0),
        }))),
        Err(e) => Err(AppError::Internal(format!("Sync failed: {}", e))),
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

    /// A non-owner gets the shape of a backend, never anything that can carry a
    /// credential. An argv is one of those: the dashboard's own Connect flow
    /// writes `["-y", "mcp-remote", url, "--header", "Authorization: Bearer …"]`
    /// into exactly this field.
    #[test]
    fn a_non_admin_gets_no_env_no_headers_and_no_command_line() {
        let redacted = redact_backend_config(stored());
        assert!(redacted.get("env").is_none());
        assert!(redacted.get("headers").is_none());
        assert!(redacted.get("command").is_none());
        assert!(redacted.get("args").is_none());
        assert!(redacted.get("url").is_none());

        let redacted = redact_backend_config(json!({
            "url": "https://example.test/sse?token=sk-live-1234",
            "headers": {"Authorization": "Bearer sk-live-1234"},
        }));
        assert!(redacted.get("url").is_none());
        assert!(redacted.get("headers").is_none());
    }

    #[test]
    fn the_audit_trails_own_backend_name_is_not_registrable() {
        // `api::mcp::Target::backend_name` files the gateway's own tool calls
        // under `gateway`, and migration 012 deletes every row filed under it.
        assert!(validate_backend_name("gateway").is_err());
        assert!(validate_backend_name("GATEWAY").is_err());
        assert!(validate_backend_name("  gateway  ").is_err());

        assert!(validate_backend_name("gateway-of-gateways").is_ok());
        assert!(validate_backend_name("filesystem").is_ok());
    }

    #[test]
    fn a_backend_name_cannot_carry_the_namespace_separator_or_be_blank() {
        assert!(validate_backend_name("").is_err());
        assert!(validate_backend_name("   ").is_err());
        assert!(validate_backend_name("my__backend").is_err());
        assert!(validate_backend_name(&"n".repeat(256)).is_err());
        assert!(validate_backend_name(&"n".repeat(255)).is_ok());
    }

    /// The list exists so a stdio backend cannot read the gateway's own
    /// secrets out of the environment it inherits. A variable the server
    /// starts reading and nobody adds here is a silent regression, so the
    /// test reads `src/` rather than trusting the list.
    #[test]
    fn the_gateways_own_secrets_are_stripped_from_a_backend() {
        const NOT_SECRET: &[&str] = &[
            "RUST_LOG",
            "LISTEN_ADDR",
            "CARGO_PKG_VERSION",
            "UPDATE_CHECK_REPO",
            "UPDATE_CHECK_DISABLED",
        ];

        let mut missing = Vec::new();
        for entry in walk_rs(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
            let body = std::fs::read_to_string(&entry).unwrap();
            for (_, rest) in body
                .match_indices("env::var(\"")
                .map(|(i, m)| (i, &body[i + m.len()..]))
            {
                let Some(end) = rest.find('"') else { continue };
                let name = &rest[..end];
                if NOT_SECRET.contains(&name) || GATEWAY_ONLY_ENV.contains(&name) {
                    continue;
                }
                missing.push(format!("{name} (read in {})", entry.display()));
            }
        }
        assert!(
            missing.is_empty(),
            "the server reads these and they are neither listed in GATEWAY_ONLY_ENV nor \
             marked harmless: {missing:?}"
        );
    }

    fn walk_rs(dir: std::path::PathBuf) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk_rs(path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        out
    }
}

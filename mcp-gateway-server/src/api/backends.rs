use axum::{
    extract::{Path, State},
    routing::{get, post, delete},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState, register_discovered_tools};
use super::auth::{Claims, require_admin};

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
        .route("/backends/:id", delete(delete_backend).patch(update_backend))
        .route("/backends/:id/sync", post(sync_backend))
}

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
    for (backend_id, name, transport, config, risk_category, is_enabled, health_status, last_health_check, created_at) in backends {
        let (tool_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1"
        )
        .bind(backend_id)
        .fetch_one(&state.db)
        .await?;

        let config = if is_admin { config } else { redact_backend_config(config) };

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
        return Err(AppError::BadRequest("Transport must be 'stdio', 'streamable-http', 'sse', or 'agent'".into()));
    }

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
        "stdio" => Some(state.backend_manager.spawn_backend(backend_id, &req.name, &req.config).await),
        "streamable-http" => Some(crate::backends::BackendManager::discover_http_tools(&req.name, &req.config).await),
        "sse" => Some(crate::backends::BackendManager::discover_sse_tools(&req.name, &req.config).await),
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
        config: req.config,
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
    let row: Option<(String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT name, transport, config FROM backends WHERE backend_id = $1"
    ).bind(id).fetch_optional(&state.db).await?;

    let (name, transport, current_config) = match row {
        Some(r) => r,
        None => return Err(AppError::NotFound("Backend not found".into())),
    };

    if let Some(is_enabled) = req.is_enabled {
        sqlx::query("UPDATE backends SET is_enabled = $1 WHERE backend_id = $2")
            .bind(is_enabled).bind(id).execute(&state.db).await?;

        if is_enabled {
            let config = req.config.as_ref().unwrap_or(&current_config);
            let result = match transport.as_str() {
                "stdio" => Some(state.backend_manager.spawn_backend(id, &name, config).await),
                "streamable-http" => Some(crate::backends::BackendManager::discover_http_tools(&name, config).await),
                "sse" => Some(crate::backends::BackendManager::discover_sse_tools(&name, config).await),
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
            let _ = sqlx::query("UPDATE tool_registry SET is_enabled = FALSE WHERE backend_id = $1")
                .bind(id).execute(&state.db).await;
            let _ = sqlx::query("UPDATE backends SET health_status = 'idle', last_health_check = NOW() WHERE backend_id = $1")
                .bind(id).execute(&state.db).await;
        }
    }
    if let Some(config) = &req.config {
        sqlx::query("UPDATE backends SET config = $1 WHERE backend_id = $2")
            .bind(config).bind(id).execute(&state.db).await?;
    }
    if let Some(risk_category) = &req.risk_category {
        sqlx::query("UPDATE backends SET risk_category = $1 WHERE backend_id = $2")
            .bind(risk_category).bind(id).execute(&state.db).await?;
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
        .bind(id).execute(&state.db).await?;

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
        "SELECT name, transport, config, is_enabled FROM backends WHERE backend_id = $1"
    ).bind(id).fetch_optional(&state.db).await?;

    let (name, transport, config, is_enabled) = match row {
        Some(r) => r,
        None => return Err(AppError::NotFound("Backend not found".into())),
    };

    if !is_enabled {
        return Err(AppError::BadRequest("Cannot sync a disabled backend".into()));
    }

    let result = match transport.as_str() {
        "stdio" => {
            state.backend_manager.stop_backend(&id).await;
            state.backend_manager.spawn_backend(id, &name, &config).await
        }
        "streamable-http" => {
            crate::backends::BackendManager::discover_http_tools(&name, &config).await
        }
        "sse" => {
            crate::backends::BackendManager::discover_sse_tools(&name, &config).await
        }
        "agent" => {
            // Extract agent_id from the backend config
            let agent_id = config
                .get("agent_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::Internal("Agent backend missing agent_id in config".into()))?
                .to_string();

            // Send a resync request to the connected agent
            match state.agent_registry.request_resync(&agent_id).await {
                Ok(()) => {
                    // The agent will re-send its register message which updates tools in DB
                    // Give the agent a moment to respond, then return current tool count
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let (tool_count,): (i64,) = sqlx::query_as(
                        "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1"
                    ).bind(id).fetch_one(&state.db).await?;

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
        _ => return Err(AppError::BadRequest(format!("Unsupported transport: {}", transport))),
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

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::Claims;

#[derive(Serialize)]
pub struct AppNode {
    pub application: String,
    pub is_connected: bool,
    pub last_seen: Option<String>,
    pub call_count: i64,
}

#[derive(Serialize)]
pub struct BackendNode {
    pub backend_name: String,
    pub transport: String,
    pub health_status: String,
    pub tool_count: i64,
}

#[derive(Serialize)]
pub struct ToolNode {
    pub tool_name: String,
    pub backend_name: String,
    pub risk_category: Option<String>,
    pub call_count: i64,
    pub last_call: Option<String>,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub call_count: i64,
    pub last_call: Option<String>,
}

#[derive(Serialize)]
pub struct UsageGraph {
    pub applications: Vec<AppNode>,
    pub backends: Vec<BackendNode>,
    pub tools: Vec<ToolNode>,
    pub app_to_backend: Vec<GraphEdge>,
    pub backend_to_tool: Vec<GraphEdge>,
}

#[derive(Deserialize)]
pub struct UsageQuery {
    pub user_id: Option<String>,
    pub range: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectionStatus {
    pub application: String,
    pub is_connected: bool,
    pub last_seen: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/usage/graph", get(usage_graph))
        .route("/usage/connections", get(usage_connections))
}

fn range_to_interval(range: &str) -> &str {
    match range {
        "24h" => "24 hours",
        "7d" => "7 days",
        "30d" => "30 days",
        _ => "7 days",
    }
}

async fn usage_graph(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageGraph>, AppError> {
    let is_admin = claims.roles.contains(&"owner".to_string());
    let target_user_id: Uuid = if is_admin {
        query.user_id.as_deref().unwrap_or(&claims.sub)
    } else {
        &claims.sub
    }
    .parse()
    .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    let interval = range_to_interval(query.range.as_deref().unwrap_or("7d"));

    // Applications from api_keys for this user
    let app_rows: Vec<(Option<String>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT application, last_used FROM api_keys WHERE user_id = $1 AND application IS NOT NULL"
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let now = chrono::Utc::now();
    let five_min_ago = now - chrono::Duration::minutes(5);

    // Get call counts per application in range
    let app_counts: Vec<(Option<String>, i64)> = sqlx::query_as(
        &format!(
            "SELECT application, COUNT(*) FROM audit_events WHERE user_id = $1 AND timestamp > NOW() - INTERVAL '{}' GROUP BY application",
            interval
        )
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let applications: Vec<AppNode> = app_rows.into_iter().filter_map(|(app, last_used)| {
        let app = app?;
        let is_connected = last_used.map(|t| t > five_min_ago).unwrap_or(false);
        let call_count = app_counts.iter()
            .find(|(a, _)| a.as_deref() == Some(&*app))
            .map(|(_, c)| *c)
            .unwrap_or(0);
        Some(AppNode {
            application: app,
            is_connected,
            last_seen: last_used.map(|t| t.to_rfc3339()),
            call_count,
        })
    }).collect();

    // Backends
    let backends: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT b.name, b.transport, b.health_status, COUNT(t.tool_id)
         FROM backends b LEFT JOIN tool_registry t ON t.backend_id = b.backend_id AND t.is_enabled = TRUE
         WHERE b.is_enabled = TRUE
         GROUP BY b.name, b.transport, b.health_status"
    )
    .fetch_all(&state.db)
    .await?;

    let backend_nodes: Vec<BackendNode> = backends.into_iter().map(|(name, transport, health, tool_count)| {
        BackendNode { backend_name: name, transport, health_status: health, tool_count }
    }).collect();

    // Tools: start from tool_registry (always visible), enrich with audit call counts + last_call
    let tool_rows: Vec<(String, String, Option<String>, i64, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        &format!(
            "SELECT t.tool_name, b.name as backend_name, t.risk_category,
                    COALESCE(ae.cnt, 0) as call_count, ae.last_call
             FROM tool_registry t
             JOIN backends b ON t.backend_id = b.backend_id
             LEFT JOIN (
                 SELECT tool_name, COUNT(*) as cnt, MAX(timestamp) as last_call
                 FROM audit_events
                 WHERE user_id = $1 AND timestamp > NOW() - INTERVAL '{}'
                 GROUP BY tool_name
             ) ae ON ae.tool_name = t.tool_name
             WHERE t.is_enabled = TRUE AND b.is_enabled = TRUE
             ORDER BY call_count DESC, t.tool_name
             LIMIT 100",
            interval
        )
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let tools: Vec<ToolNode> = tool_rows.into_iter().map(|(tool_name, backend_name, risk_category, call_count, last_call)| {
        ToolNode { tool_name, backend_name, risk_category, call_count, last_call: last_call.map(|t| t.to_rfc3339()) }
    }).collect();

    // Edges: app → backend
    let app_backend_edges: Vec<(Option<String>, String, i64, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        &format!(
            "SELECT application, backend_name, COUNT(*) as cnt, MAX(timestamp) as last_call
             FROM audit_events
             WHERE user_id = $1 AND timestamp > NOW() - INTERVAL '{}' AND application IS NOT NULL
             GROUP BY application, backend_name",
            interval
        )
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let app_to_backend: Vec<GraphEdge> = app_backend_edges.into_iter().filter_map(|(app, backend, cnt, last)| {
        Some(GraphEdge {
            source: app?,
            target: backend,
            call_count: cnt,
            last_call: last.map(|t| t.to_rfc3339()),
        })
    }).collect();

    // Edges: backend → tool (from registry + audit counts)
    let backend_tool_edges: Vec<(String, String, i64, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        &format!(
            "SELECT b.name as backend_name, t.tool_name,
                    COALESCE(ae.cnt, 0) as call_count, ae.last_call
             FROM tool_registry t
             JOIN backends b ON t.backend_id = b.backend_id
             LEFT JOIN (
                 SELECT tool_name, COUNT(*) as cnt, MAX(timestamp) as last_call
                 FROM audit_events
                 WHERE user_id = $1 AND timestamp > NOW() - INTERVAL '{}'
                 GROUP BY tool_name
             ) ae ON ae.tool_name = t.tool_name
             WHERE t.is_enabled = TRUE AND b.is_enabled = TRUE
             ORDER BY call_count DESC
             LIMIT 50",
            interval
        )
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let backend_to_tool: Vec<GraphEdge> = backend_tool_edges.into_iter().map(|(backend, tool, cnt, last)| {
        GraphEdge {
            source: backend,
            target: tool,
            call_count: cnt,
            last_call: last.map(|t| t.to_rfc3339()),
        }
    }).collect();

    Ok(Json(UsageGraph {
        applications,
        backends: backend_nodes,
        tools,
        app_to_backend,
        backend_to_tool,
    }))
}

#[derive(Deserialize)]
pub struct ConnectionQuery {
    pub user_id: Option<String>,
}

async fn usage_connections(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<ConnectionQuery>,
) -> Result<Json<Vec<ConnectionStatus>>, AppError> {
    let is_admin = claims.roles.contains(&"owner".to_string());
    let target_user_id: Uuid = if is_admin {
        query.user_id.as_deref().unwrap_or(&claims.sub)
    } else {
        &claims.sub
    }
    .parse()
    .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    let rows: Vec<(Option<String>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT application, last_used FROM api_keys WHERE user_id = $1 AND application IS NOT NULL"
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let now = chrono::Utc::now();
    let five_min_ago = now - chrono::Duration::minutes(5);

    let result: Vec<ConnectionStatus> = rows.into_iter().filter_map(|(app, last_used)| {
        let app = app?;
        let is_connected = last_used.map(|t| t > five_min_ago).unwrap_or(false);
        Some(ConnectionStatus {
            application: app,
            is_connected,
            last_seen: last_used.map(|t| t.to_rfc3339()),
        })
    }).collect();

    Ok(Json(result))
}

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::Claims;

#[derive(Serialize)]
pub struct ToolResponse {
    pub tool_id: String,
    pub tool_name: String,
    pub backend_name: String,
    pub original_name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub risk_category: Option<String>,
    pub is_enabled: bool,
    pub last_seen: String,
    pub call_count_24h: i64,
}

#[derive(Deserialize)]
pub struct ToolQuery {
    pub backend: Option<String>,
    pub risk_category: Option<String>,
    pub enabled: Option<bool>,
    pub search: Option<String>,
    pub calls_range: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateToolRequest {
    pub is_enabled: Option<bool>,
    pub description: Option<String>,
    pub risk_category: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tools", get(list_tools))
        .route("/tools/:id", axum::routing::patch(update_tool))
}

async fn list_tools(
    State(state): State<AppState>,
    _claims: Claims,
    Query(query): Query<ToolQuery>,
) -> Result<Json<Vec<ToolResponse>>, AppError> {
    let interval = match query.calls_range.as_deref() {
        Some("7d") => "7 days",
        Some("30d") => "30 days",
        _ => "24 hours",
    };

    let base_sql = format!(
        "SELECT t.tool_id, t.tool_name, b.name as backend_name, t.original_name, t.description, t.input_schema, t.risk_category, t.is_enabled, t.last_seen,
         COALESCE((SELECT COUNT(*) FROM audit_events a WHERE a.tool_name = t.tool_name AND a.timestamp > NOW() - INTERVAL '{}'), 0) as call_count
         FROM tool_registry t
         JOIN backends b ON t.backend_id = b.backend_id
         WHERE 1=1",
        interval
    );
    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(base_sql);

    if let Some(ref backend) = query.backend {
        qb.push(" AND b.name = ");
        qb.push_bind(backend.clone());
    }
    if let Some(ref risk) = query.risk_category {
        qb.push(" AND t.risk_category = ");
        qb.push_bind(risk.clone());
    }
    if let Some(enabled) = query.enabled {
        qb.push(" AND t.is_enabled = ");
        qb.push_bind(enabled);
    }
    if let Some(ref search) = query.search {
        qb.push(" AND (t.tool_name ILIKE '%' || ");
        qb.push_bind(search.clone());
        qb.push(" || '%' OR t.description ILIKE '%' || ");
        qb.push_bind(search.clone());
        qb.push(" || '%')");
    }

    qb.push(" ORDER BY t.tool_name");

    let tools: Vec<(Uuid, String, String, String, Option<String>, Option<serde_json::Value>, Option<String>, bool, chrono::DateTime<chrono::Utc>, i64)> = qb.build_query_as()
        .fetch_all(&state.db)
        .await?;

    let result: Vec<ToolResponse> = tools.into_iter().map(|(tool_id, tool_name, backend_name, original_name, description, input_schema, risk_category, is_enabled, last_seen, call_count)| {
        ToolResponse {
            tool_id: tool_id.to_string(),
            tool_name,
            backend_name,
            original_name,
            description,
            input_schema,
            risk_category,
            is_enabled,
            last_seen: last_seen.to_rfc3339(),
            call_count_24h: call_count,
        }
    }).collect();

    Ok(Json(result))
}

async fn update_tool(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateToolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    super::auth::require_admin(&claims)?;

    if let Some(is_enabled) = req.is_enabled {
        sqlx::query("UPDATE tool_registry SET is_enabled = $1 WHERE tool_id = $2")
            .bind(is_enabled).bind(id).execute(&state.db).await?;
    }
    if let Some(description) = &req.description {
        sqlx::query("UPDATE tool_registry SET description = $1 WHERE tool_id = $2")
            .bind(description).bind(id).execute(&state.db).await?;
    }
    if let Some(risk_category) = &req.risk_category {
        sqlx::query("UPDATE tool_registry SET risk_category = $1 WHERE tool_id = $2")
            .bind(risk_category).bind(id).execute(&state.db).await?;

        // Propagate the new risk category to all historical audit events for this tool
        sqlx::query(
            "UPDATE audit_events SET risk_category = $1
             WHERE tool_name = (SELECT tool_name FROM tool_registry WHERE tool_id = $2)"
        )
            .bind(risk_category).bind(id).execute(&state.db).await?;
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

use axum::{
    extract::{Query, State},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::Claims;

#[derive(sqlx::FromRow)]
struct AuditEventRow {
    event_id: Uuid,
    timestamp: chrono::DateTime<chrono::Utc>,
    trace_id: Uuid,
    session_id: Option<String>,
    user_id: Option<Uuid>,
    client_id: Option<String>,
    tool_name: String,
    backend_name: String,
    risk_category: Option<String>,
    request_hash: Option<String>,
    response_hash: Option<String>,
    duration_ms: Option<f64>,
    status: String,
    error_message: Option<String>,
    policy_decision: Option<String>,
    policy_id: Option<Uuid>,
    risk_flags: serde_json::Value,
    metadata: serde_json::Value,
    application: Option<String>,
}

#[derive(Serialize)]
pub struct AuditEventResponse {
    pub event_id: String,
    pub timestamp: String,
    pub trace_id: String,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub client_id: Option<String>,
    pub tool_name: String,
    pub backend_name: String,
    pub risk_category: Option<String>,
    pub request_hash: Option<String>,
    pub response_hash: Option<String>,
    pub duration_ms: Option<f64>,
    pub status: String,
    pub error_message: Option<String>,
    pub policy_decision: Option<String>,
    pub policy_id: Option<String>,
    pub risk_flags: serde_json::Value,
    pub metadata: serde_json::Value,
    pub application: Option<String>,
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub tool_name: Option<String>,
    pub backend: Option<String>,
    pub status: Option<String>,
    pub user_id: Option<String>,
    pub risk_category: Option<String>,
    pub policy_decision: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub application: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct AuditExportResponse {
    pub events: Vec<AuditEventResponse>,
    pub total: i64,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audit", get(query_audit).delete(clear_audit))
        .route("/audit/export", get(export_audit))
        .route("/audit/stats", get(audit_stats))
}

async fn query_audit(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditExportResponse>, AppError> {
    let limit = query.limit.unwrap_or(50).min(500);
    let offset = query.offset.unwrap_or(0);

    // Non-admin users can only see their own events
    let user_filter = if !claims.roles.contains(&"owner".to_string()) {
        Some(claims.sub.clone())
    } else {
        query.user_id.clone()
    };

    // Build dynamic query with sqlx::QueryBuilder
    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT event_id, timestamp, trace_id, session_id, user_id, client_id, tool_name, backend_name, risk_category, request_hash, response_hash, duration_ms, status, error_message, policy_decision, policy_id, COALESCE(risk_flags, '[]'::jsonb) as risk_flags, COALESCE(metadata, '{}'::jsonb) as metadata, application FROM audit_events WHERE 1=1"
    );

    if let Some(ref tool) = query.tool_name {
        qb.push(" AND tool_name ILIKE '%' || ");
        qb.push_bind(tool.clone());
        qb.push(" || '%'");
    }
    if let Some(ref backend) = query.backend {
        qb.push(" AND backend_name = ");
        qb.push_bind(backend.clone());
    }
    if let Some(ref status) = query.status {
        qb.push(" AND status = ");
        qb.push_bind(status.clone());
    }
    if let Some(ref uid) = user_filter {
        qb.push(" AND user_id = ");
        qb.push_bind(Uuid::parse_str(uid).unwrap_or_default());
    }
    if let Some(ref risk) = query.risk_category {
        qb.push(" AND risk_category = ");
        qb.push_bind(risk.clone());
    }
    if let Some(ref decision) = query.policy_decision {
        qb.push(" AND policy_decision = ");
        qb.push_bind(decision.clone());
    }
    if let Some(ref app) = query.application {
        qb.push(" AND application = ");
        qb.push_bind(app.clone());
    }
    if let Some(ref from) = query.from {
        if let Ok(from_dt) = chrono::DateTime::parse_from_rfc3339(from) {
            qb.push(" AND timestamp >= ");
            qb.push_bind(from_dt.with_timezone(&chrono::Utc));
        }
    }
    if let Some(ref to) = query.to {
        if let Ok(to_dt) = chrono::DateTime::parse_from_rfc3339(to) {
            qb.push(" AND timestamp <= ");
            qb.push_bind(to_dt.with_timezone(&chrono::Utc));
        }
    }

    // Count query (same filters)
    let mut count_qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT COUNT(*) FROM audit_events WHERE 1=1"
    );
    if let Some(ref tool) = query.tool_name {
        count_qb.push(" AND tool_name ILIKE '%' || ");
        count_qb.push_bind(tool.clone());
        count_qb.push(" || '%'");
    }
    if let Some(ref backend) = query.backend {
        count_qb.push(" AND backend_name = ");
        count_qb.push_bind(backend.clone());
    }
    if let Some(ref status) = query.status {
        count_qb.push(" AND status = ");
        count_qb.push_bind(status.clone());
    }
    if let Some(ref uid) = user_filter {
        count_qb.push(" AND user_id = ");
        count_qb.push_bind(Uuid::parse_str(uid).unwrap_or_default());
    }
    if let Some(ref risk) = query.risk_category {
        count_qb.push(" AND risk_category = ");
        count_qb.push_bind(risk.clone());
    }
    if let Some(ref decision) = query.policy_decision {
        count_qb.push(" AND policy_decision = ");
        count_qb.push_bind(decision.clone());
    }
    if let Some(ref app) = query.application {
        count_qb.push(" AND application = ");
        count_qb.push_bind(app.clone());
    }
    if let Some(ref from) = query.from {
        if let Ok(from_dt) = chrono::DateTime::parse_from_rfc3339(from) {
            count_qb.push(" AND timestamp >= ");
            count_qb.push_bind(from_dt.with_timezone(&chrono::Utc));
        }
    }
    if let Some(ref to) = query.to {
        if let Ok(to_dt) = chrono::DateTime::parse_from_rfc3339(to) {
            count_qb.push(" AND timestamp <= ");
            count_qb.push_bind(to_dt.with_timezone(&chrono::Utc));
        }
    }

    qb.push(" ORDER BY timestamp DESC LIMIT ");
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let events: Vec<AuditEventRow> = qb.build_query_as().fetch_all(&state.db).await?;
    let total: (i64,) = count_qb.build_query_as().fetch_one(&state.db).await?;

    let result: Vec<AuditEventResponse> = events.into_iter().map(|e| {
        AuditEventResponse {
            event_id: e.event_id.to_string(),
            timestamp: e.timestamp.to_rfc3339(),
            trace_id: e.trace_id.to_string(),
            session_id: e.session_id,
            user_id: e.user_id.map(|u| u.to_string()),
            client_id: e.client_id,
            tool_name: e.tool_name,
            backend_name: e.backend_name,
            risk_category: e.risk_category,
            request_hash: e.request_hash,
            response_hash: e.response_hash,
            duration_ms: e.duration_ms,
            status: e.status,
            error_message: e.error_message,
            policy_decision: e.policy_decision,
            policy_id: e.policy_id.map(|p| p.to_string()),
            risk_flags: e.risk_flags,
            metadata: e.metadata,
            application: e.application,
        }
    }).collect();

    Ok(Json(AuditExportResponse {
        events: result,
        total: total.0,
    }))
}

async fn export_audit(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<AuditExportResponse>, AppError> {
    super::auth::require_admin(&claims)?;

    let events: Vec<AuditEventRow> = sqlx::query_as(
        "SELECT event_id, timestamp, trace_id, session_id, user_id, client_id, tool_name, backend_name, risk_category, request_hash, response_hash, duration_ms, status, error_message, policy_decision, policy_id, COALESCE(risk_flags, '[]'::jsonb) as risk_flags, COALESCE(metadata, '{}'::jsonb) as metadata, application
         FROM audit_events
         ORDER BY timestamp DESC
         LIMIT 10000"
    )
    .fetch_all(&state.db)
    .await?;

    let total = events.len() as i64;

    let result: Vec<AuditEventResponse> = events.into_iter().map(|e| {
        AuditEventResponse {
            event_id: e.event_id.to_string(),
            timestamp: e.timestamp.to_rfc3339(),
            trace_id: e.trace_id.to_string(),
            session_id: e.session_id,
            user_id: e.user_id.map(|u| u.to_string()),
            client_id: e.client_id,
            tool_name: e.tool_name,
            backend_name: e.backend_name,
            risk_category: e.risk_category,
            request_hash: e.request_hash,
            response_hash: e.response_hash,
            duration_ms: e.duration_ms,
            status: e.status,
            error_message: e.error_message,
            policy_decision: e.policy_decision,
            policy_id: e.policy_id.map(|p| p.to_string()),
            risk_flags: e.risk_flags,
            metadata: e.metadata,
            application: e.application,
        }
    }).collect();

    Ok(Json(AuditExportResponse {
        events: result,
        total,
    }))
}

#[derive(Serialize)]
pub struct AuditStats {
    pub total_events: i64,
    pub events_24h: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub denied_count: i64,
    pub avg_duration_ms: f64,
    pub top_tools: Vec<ToolStat>,
    pub status_breakdown: Vec<StatusStat>,
    pub hourly_volume: Vec<HourlyStat>,
}

#[derive(Serialize)]
pub struct ToolStat {
    pub tool_name: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct StatusStat {
    pub status: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct HourlyStat {
    pub hour: String,
    pub count: i64,
}

async fn clear_audit(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    super::auth::require_admin(&claims)?;

    sqlx::query("TRUNCATE audit_events")
        .execute(&state.db)
        .await?;

    // Reset prometheus counters by re-creating the collector
    state.metrics.tool_calls_total.reset();
    state.metrics.tool_call_duration.reset();
    state.metrics.policy_decisions_total.reset();
    state.metrics.audit_events_total.reset();
    state.metrics.bytes_in_total.reset();
    state.metrics.bytes_out_total.reset();

    tracing::info!(user = %claims.username, "Audit events cleared and metrics reset");

    Ok(Json(serde_json::json!({ "status": "cleared" })))
}

async fn audit_stats(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<AuditStats>, AppError> {
    let (total_events,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&state.db).await?;

    let (events_24h,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_one(&state.db).await?;

    let (success_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE status = 'success'"
    ).fetch_one(&state.db).await?;

    let (error_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE status = 'error'"
    ).fetch_one(&state.db).await?;

    let (denied_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE status = 'denied'"
    ).fetch_one(&state.db).await?;

    let avg_duration: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT AVG(duration_ms) FROM audit_events WHERE duration_ms IS NOT NULL"
    ).fetch_optional(&state.db).await?;

    let top_tools: Vec<(String, i64)> = sqlx::query_as(
        "SELECT tool_name, COUNT(*) as cnt FROM audit_events GROUP BY tool_name ORDER BY cnt DESC LIMIT 10"
    ).fetch_all(&state.db).await?;

    let status_breakdown: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM audit_events GROUP BY status"
    ).fetch_all(&state.db).await?;

    let hourly_volume: Vec<(chrono::DateTime<chrono::Utc>, i64)> = sqlx::query_as(
        "SELECT date_trunc('hour', timestamp) as hr, COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours' GROUP BY hr ORDER BY hr"
    ).fetch_all(&state.db).await?;

    Ok(Json(AuditStats {
        total_events,
        events_24h,
        success_count,
        error_count,
        denied_count,
        avg_duration_ms: avg_duration.and_then(|(v,)| v).unwrap_or(0.0),
        top_tools: top_tools.into_iter().map(|(tool_name, count)| ToolStat { tool_name, count }).collect(),
        status_breakdown: status_breakdown.into_iter().map(|(status, count)| StatusStat { status, count }).collect(),
        hourly_volume: hourly_volume.into_iter().map(|(hour, count)| HourlyStat { hour: hour.to_rfc3339(), count }).collect(),
    }))
}

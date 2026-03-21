use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::{AppError, AppState};
use super::auth::Claims;

#[derive(Serialize)]
pub struct MetricsSummary {
    pub total_tool_calls: i64,
    pub calls_last_24h: i64,
    pub active_backends: i64,
    pub total_backends: i64,
    pub total_tools: i64,
    pub enabled_tools: i64,
    pub total_users: i64,
    pub active_policies: i64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
    pub top_tools_24h: Vec<ToolMetric>,
    pub backend_health: Vec<BackendHealth>,
    pub latency_percentiles: LatencyPercentiles,
    pub calls_by_risk: Vec<RiskMetric>,
    pub hourly_volume: Vec<HourlyVolume>,
}

#[derive(Serialize)]
pub struct ToolMetric {
    pub tool_name: String,
    pub call_count: i64,
    pub avg_duration_ms: f64,
    pub error_count: i64,
}

#[derive(Serialize)]
pub struct BackendHealth {
    pub name: String,
    pub status: String,
    pub tool_count: i64,
}

#[derive(Serialize)]
pub struct LatencyPercentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

#[derive(Serialize)]
pub struct RiskMetric {
    pub risk_category: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct HourlyVolume {
    pub hour: String,
    pub count: i64,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metrics/summary", get(metrics_summary))
}

async fn metrics_summary(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<MetricsSummary>, AppError> {
    let (total_tool_calls,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&state.db).await?;

    let (calls_last_24h,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_one(&state.db).await?;

    let (active_backends,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE AND health_status = 'healthy'"
    ).fetch_one(&state.db).await?;

    let (total_backends,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM backends")
        .fetch_one(&state.db).await?;

    let (total_tools,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tool_registry")
        .fetch_one(&state.db).await?;

    let (enabled_tools,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tool_registry WHERE is_enabled = TRUE"
    ).fetch_one(&state.db).await?;

    let (total_users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db).await?;

    let (active_policies,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM policies WHERE is_active = TRUE"
    ).fetch_one(&state.db).await?;

    let avg_lat: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT AVG(duration_ms) FROM audit_events WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_optional(&state.db).await?;

    let (error_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events WHERE status = 'error' AND timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_one(&state.db).await?;

    let error_rate = if calls_last_24h > 0 {
        error_count as f64 / calls_last_24h as f64 * 100.0
    } else {
        0.0
    };

    let top_tools: Vec<(String, i64, Option<f64>, i64)> = sqlx::query_as(
        "SELECT tool_name, COUNT(*) as cnt, AVG(duration_ms), SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END)
         FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours'
         GROUP BY tool_name ORDER BY cnt DESC LIMIT 10"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let backend_health: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT b.name, b.health_status, COUNT(t.tool_id) FROM backends b LEFT JOIN tool_registry t ON b.backend_id = t.backend_id GROUP BY b.name, b.health_status ORDER BY b.name"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let calls_by_risk: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT COALESCE(t.risk_category, a.risk_category) as risk, COUNT(*)
         FROM audit_events a
         LEFT JOIN tool_registry t ON t.tool_name = a.tool_name
         WHERE a.timestamp > NOW() - INTERVAL '24 hours'
         GROUP BY risk"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let hourly_volume: Vec<(chrono::DateTime<chrono::Utc>, i64)> = sqlx::query_as(
        "SELECT date_trunc('hour', timestamp) AS hour, COUNT(*) AS cnt \
         FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours' \
         GROUP BY hour ORDER BY hour"
    ).fetch_all(&state.db).await.unwrap_or_default();

    // Approximate percentiles
    let percentiles = compute_percentiles(&state.db).await;

    Ok(Json(MetricsSummary {
        total_tool_calls,
        calls_last_24h,
        active_backends,
        total_backends,
        total_tools,
        enabled_tools,
        total_users,
        active_policies,
        avg_latency_ms: avg_lat.and_then(|(v,)| v).unwrap_or(0.0),
        error_rate,
        top_tools_24h: top_tools.into_iter().map(|(tool_name, call_count, avg, errors)| ToolMetric {
            tool_name,
            call_count,
            avg_duration_ms: avg.unwrap_or(0.0),
            error_count: errors,
        }).collect(),
        backend_health: backend_health.into_iter().map(|(name, status, tool_count)| BackendHealth {
            name, status, tool_count,
        }).collect(),
        latency_percentiles: percentiles,
        calls_by_risk: calls_by_risk.into_iter().map(|(risk_category, count)| RiskMetric {
            risk_category: risk_category.unwrap_or_else(|| "unknown".into()),
            count,
        }).collect(),
        hourly_volume: hourly_volume.into_iter().map(|(hour, count)| HourlyVolume {
            hour: hour.format("%H:%M").to_string(),
            count,
        }).collect(),
    }))
}

async fn compute_percentiles(db: &sqlx::PgPool) -> LatencyPercentiles {
    let p50: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY duration_ms) FROM audit_events WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_optional(db).await.ok().flatten();

    let p95: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) FROM audit_events WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_optional(db).await.ok().flatten();

    let p99: Option<(Option<f64>,)> = sqlx::query_as(
        "SELECT percentile_cont(0.99) WITHIN GROUP (ORDER BY duration_ms) FROM audit_events WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'"
    ).fetch_optional(db).await.ok().flatten();

    LatencyPercentiles {
        p50: p50.and_then(|(v,)| v).unwrap_or(0.0),
        p95: p95.and_then(|(v,)| v).unwrap_or(0.0),
        p99: p99.and_then(|(v,)| v).unwrap_or(0.0),
    }
}

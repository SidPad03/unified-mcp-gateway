use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::auth::Claims;
use crate::{AppError, AppState};

#[derive(Serialize)]
pub struct MetricsSummary {
    /// Every row the audit trail currently holds, at any age. The only figure
    /// here that the range does not touch.
    pub total_tool_calls: i64,
    /// The range these figures were computed over, echoed back so the client
    /// can label them without trusting its own request to have been honoured.
    pub range: String,
    pub calls_in_range: i64,
    pub active_backends: i64,
    pub total_backends: i64,
    pub total_tools: i64,
    pub enabled_tools: i64,
    pub total_users: i64,
    pub active_policies: i64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
    pub top_tools: Vec<ToolMetric>,
    pub backend_health: Vec<BackendHealth>,
    pub latency_percentiles: LatencyPercentiles,
    pub calls_by_risk: Vec<RiskMetric>,
    pub volume: Vec<VolumePoint>,
    /// `hour` or `day` — how wide one point of `volume` is. Thirty days of
    /// hourly points is 720 of them in a 190px chart; the client also needs to
    /// know whether to label a tick with a time or a date.
    pub volume_bucket: String,
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
pub struct VolumePoint {
    /// RFC 3339, the same as `/audit/stats` sends. A bare "%H:%M" is not a
    /// date: `new Date("05:00")` is Invalid Date, so the chart's axis read
    /// "Invalid Date" and its tooltip fell back to the em dash. The client
    /// needs the full instant anyway, to render it in the reader's own
    /// timezone rather than the server's.
    pub bucket: String,
    pub count: i64,
}

#[derive(Deserialize)]
pub struct MetricsQuery {
    pub range: Option<String>,
}

/// The windows the dashboard offers, and the only strings that reach SQL.
///
/// Every query below interpolates the interval with `format!` rather than
/// binding it, because Postgres will not take a placeholder inside an
/// `INTERVAL` literal. That is only safe while the value comes from this
/// whitelist — an unknown range falls back to 24 hours rather than passing
/// anything through. `usage.rs` and `tools.rs` resolve their ranges the same
/// way, and the three lists have to stay in step or the pages disagree about
/// what "7d" means.
fn resolve_range(range: Option<&str>) -> (&'static str, &'static str, &'static str) {
    match range.unwrap_or("24h") {
        "7d" => ("7d", "7 days", "hour"),
        "30d" => ("30d", "30 days", "day"),
        // 24h, and anything unrecognised.
        _ => ("24h", "24 hours", "hour"),
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/metrics/summary", get(metrics_summary))
}

/// Owner-only, because every figure below is a deployment-wide aggregate.
///
/// `/audit/stats` is the per-user view of the same rows and scopes a non-owner
/// to their own `sub`; this one cannot, because `total_backends`, `total_tools`
/// and `active_policies` have no per-user meaning. Leaving it merely
/// authenticated handed any account — and any `mcpgw_` key — the whole
/// gateway's call volume and the names of its ten busiest tools.
async fn metrics_summary(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<MetricsSummary>, AppError> {
    super::auth::require_admin(&claims)?;

    let (range, interval, bucket) = resolve_range(query.range.as_deref());

    let (total_tool_calls,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&state.db)
        .await?;

    let (calls_in_range,): (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '{interval}'"
    ))
    .fetch_one(&state.db)
    .await?;

    let (active_backends,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE AND health_status = 'healthy'",
    )
    .fetch_one(&state.db)
    .await?;

    let (total_backends,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM backends")
        .fetch_one(&state.db)
        .await?;

    // Internal tools are excluded everywhere the operator counts theirs.
    let (total_tools,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM tool_registry WHERE is_internal = FALSE")
            .fetch_one(&state.db)
            .await?;

    let (enabled_tools,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tool_registry WHERE is_enabled = TRUE AND is_internal = FALSE",
    )
    .fetch_one(&state.db)
    .await?;

    let (total_users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;

    let (active_policies,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM policies WHERE is_active = TRUE")
            .fetch_one(&state.db)
            .await?;

    let avg_lat: Option<(Option<f64>,)> = sqlx::query_as(&format!(
        "SELECT AVG(duration_ms) FROM audit_events \
         WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '{interval}'"
    ))
    .fetch_optional(&state.db)
    .await?;

    // Include `tool_error` — a tool that returned isError=true is a failed call.
    // Counting only 'error' reported a 0% error rate while tools were failing.
    let (error_count,): (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM audit_events \
         WHERE status IN ('error', 'tool_error') AND timestamp > NOW() - INTERVAL '{interval}'"
    ))
    .fetch_one(&state.db)
    .await?;

    // A fraction in 0.0..=1.0, not a percentage. Both callers treat it as one:
    // the dashboard renders it through `fmt.percent`, which multiplies by 100,
    // and tones it against 0.01 / 0.05 thresholds. Returning 70.0 for a 70%
    // error rate therefore drew "7000%" and painted every non-zero rate red.
    let error_rate = if calls_in_range > 0 {
        error_count as f64 / calls_in_range as f64
    } else {
        0.0
    };

    let top_tools: Vec<(String, i64, Option<f64>, i64)> = sqlx::query_as(&format!(
        "SELECT tool_name, COUNT(*) as cnt, AVG(duration_ms), \
                SUM(CASE WHEN status IN ('error', 'tool_error') THEN 1 ELSE 0 END) \
         FROM audit_events WHERE timestamp > NOW() - INTERVAL '{interval}' \
         GROUP BY tool_name ORDER BY cnt DESC LIMIT 10"
    ))
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // `is_internal = FALSE` is not optional here: this is the same noun the
    // Backends page draws, and without the filter a connected Mac read 15 tools
    // on this panel and 6 one page over.
    let backend_health: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT b.name, b.health_status, COUNT(t.tool_id) FILTER (WHERE t.is_internal = FALSE) \
         FROM backends b LEFT JOIN tool_registry t ON b.backend_id = t.backend_id \
         GROUP BY b.name, b.health_status ORDER BY b.name",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let calls_by_risk: Vec<(Option<String>, i64)> = sqlx::query_as(&format!(
        "SELECT COALESCE(t.risk_category, a.risk_category) as risk, COUNT(*) \
         FROM audit_events a \
         LEFT JOIN tool_registry t ON t.tool_name = a.tool_name \
         WHERE a.timestamp > NOW() - INTERVAL '{interval}' \
         GROUP BY risk"
    ))
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let volume: Vec<(chrono::DateTime<chrono::Utc>, i64)> = sqlx::query_as(&format!(
        "SELECT date_trunc('{bucket}', timestamp) AS bucket, COUNT(*) AS cnt \
         FROM audit_events WHERE timestamp > NOW() - INTERVAL '{interval}' \
         GROUP BY bucket ORDER BY bucket"
    ))
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Approximate percentiles
    let percentiles = compute_percentiles(&state.db, interval).await;

    Ok(Json(MetricsSummary {
        total_tool_calls,
        range: range.to_string(),
        calls_in_range,
        active_backends,
        total_backends,
        total_tools,
        enabled_tools,
        total_users,
        active_policies,
        avg_latency_ms: avg_lat.and_then(|(v,)| v).unwrap_or(0.0),
        error_rate,
        top_tools: top_tools
            .into_iter()
            .map(|(tool_name, call_count, avg, errors)| ToolMetric {
                tool_name,
                call_count,
                avg_duration_ms: avg.unwrap_or(0.0),
                error_count: errors,
            })
            .collect(),
        backend_health: backend_health
            .into_iter()
            .map(|(name, status, tool_count)| BackendHealth {
                name,
                status,
                tool_count,
            })
            .collect(),
        latency_percentiles: percentiles,
        calls_by_risk: calls_by_risk
            .into_iter()
            .map(|(risk_category, count)| RiskMetric {
                // `unclassified` is the word the classifier, the policy editor,
                // the risk ramp and `effective_risk` all use for a tool nobody
                // has reviewed. Emitting `unknown` here put those calls in a
                // bucket the chart's whitelist did not recognise, so they were
                // dropped from the bar *and* from its percentage denominator.
                risk_category: risk_category
                    .unwrap_or_else(|| crate::api::mcp::DEFAULT_RISK.to_string()),
                count,
            })
            .collect(),
        volume: volume
            .into_iter()
            .map(|(bucket, count)| VolumePoint {
                bucket: bucket.to_rfc3339(),
                count,
            })
            .collect(),
        volume_bucket: bucket.to_string(),
    }))
}

async fn compute_percentiles(db: &sqlx::PgPool, interval: &str) -> LatencyPercentiles {
    async fn at(db: &sqlx::PgPool, interval: &str, fraction: &str) -> f64 {
        let row: Option<(Option<f64>,)> = sqlx::query_as(&format!(
            "SELECT percentile_cont({fraction}) WITHIN GROUP (ORDER BY duration_ms) \
             FROM audit_events \
             WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '{interval}'"
        ))
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
        row.and_then(|(v,)| v).unwrap_or(0.0)
    }

    LatencyPercentiles {
        p50: at(db, interval, "0.5").await,
        p95: at(db, interval, "0.95").await,
        p99: at(db, interval, "0.99").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whitelisted_ranges_reach_sql() {
        assert_eq!(resolve_range(Some("24h")), ("24h", "24 hours", "hour"));
        assert_eq!(resolve_range(Some("7d")), ("7d", "7 days", "hour"));
        assert_eq!(resolve_range(Some("30d")), ("30d", "30 days", "day"));
    }

    /// The intervals are interpolated into SQL rather than bound, so anything
    /// unrecognised — including an injection attempt — has to collapse to the
    /// default rather than travel.
    #[test]
    fn an_unknown_range_falls_back_to_the_default_window() {
        assert_eq!(resolve_range(None), ("24h", "24 hours", "hour"));
        assert_eq!(resolve_range(Some("")), ("24h", "24 hours", "hour"));
        assert_eq!(
            resolve_range(Some("1 hour'; DROP TABLE audit_events; --")),
            ("24h", "24 hours", "hour")
        );
    }

    /// A month of hourly points is 720 of them; the long window buckets by day.
    #[test]
    fn the_longest_window_buckets_by_day() {
        assert_eq!(resolve_range(Some("30d")).2, "day");
        assert_eq!(resolve_range(Some("7d")).2, "hour");
    }
}

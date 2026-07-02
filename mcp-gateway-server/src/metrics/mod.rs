pub mod collector;

pub use collector::MetricsCollector;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use prometheus::{Encoder, TextEncoder};

use crate::AppState;

pub async fn prometheus_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();
    let mut buffer = Vec::new();
    // Encode failures shouldn't panic the handler (and take a request thread
    // down with it) — return a 500 instead.
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!(error = %e, "Failed to encode Prometheus metrics");
        return (StatusCode::INTERNAL_SERVER_ERROR, String::new()).into_response();
    }
    match String::from_utf8(buffer) {
        Ok(body) => (
            [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "Prometheus metrics were not valid UTF-8");
            (StatusCode::INTERNAL_SERVER_ERROR, String::new()).into_response()
        }
    }
}

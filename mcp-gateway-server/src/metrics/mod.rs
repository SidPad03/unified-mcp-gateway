pub mod collector;

pub use collector::MetricsCollector;

use axum::extract::State;
use axum::response::IntoResponse;
use prometheus::{Encoder, TextEncoder};

use crate::AppState;

pub async fn prometheus_handler(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = state.metrics.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        String::from_utf8(buffer).unwrap(),
    )
}

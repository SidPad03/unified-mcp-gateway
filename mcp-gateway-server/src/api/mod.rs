pub mod agent_releases;
pub mod api_keys;
pub mod auth;
pub mod audit;
pub mod backends;
pub mod live;
pub mod mcp;
pub mod metrics;
pub mod policies;
pub mod roles;
pub mod tools;
pub mod usage;
pub mod users;

use axum::Router;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(tools::router())
        .merge(backends::router())
        .merge(audit::router())
        .merge(users::router())
        .merge(roles::router())
        .merge(policies::router())
        .merge(metrics::router())
        .merge(api_keys::router())
        .merge(agent_releases::router())
        .merge(usage::router())
}

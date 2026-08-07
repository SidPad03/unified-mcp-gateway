pub mod agent_auth;
pub mod api_keys;
pub mod audit;
pub mod auth;
pub mod backends;
pub mod live;
pub mod mcp;
pub mod metrics;
pub mod policies;
pub mod roles;
pub mod tools;
pub mod updates;
pub mod usage;
pub mod users;

use crate::AppState;
use axum::Router;

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
        .merge(agent_auth::router())
        .merge(usage::router())
        .merge(updates::router())
}

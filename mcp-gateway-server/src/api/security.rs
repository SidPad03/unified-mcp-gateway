//! The signals behind the dashboard's security-posture checklist.
//!
//! Deliberately raw: the endpoint reports facts, and the card decides what is a
//! pass and what is a warning. An aggregate "you are secure" verdict would be
//! manufactured assurance, because these are the handful of things the gateway
//! can check about itself and not the ones that usually go wrong.
//!
//! Owner-only. It names the accounts that hold the owner role and says whether
//! the listener is on a public interface, which is a map of where to attack.

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use super::auth::Claims;
use crate::{AppError, AppState};

#[derive(Serialize)]
pub struct SecurityPostureOwner {
    pub username: String,
    pub last_login: Option<String>,
}

#[derive(Serialize)]
pub struct SecurityPosture {
    /// What `LISTEN_ADDR` resolved to, verbatim, so the card can name it.
    pub listen_addr: String,
    pub listen_addr_public: bool,
    /// A seeded account still owes its first-login password change.
    pub admin_password_change_pending: bool,
    pub active_owner_count: i64,
    pub owners: Vec<SecurityPostureOwner>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/security/posture", get(security_posture))
}

/// Whether the listener is reachable from off the machine.
///
/// A bind address is either a wildcard (`0.0.0.0`, `::`, or a bare port) or a
/// specific interface; only loopback keeps the gateway off the network. The
/// host is compared rather than parsed as an `IpAddr` because `LISTEN_ADDR` may
/// legitimately be a hostname.
fn is_public_bind(listen_addr: &str) -> bool {
    let host = listen_addr.rsplit_once(':').map_or(listen_addr, |(h, _)| h);
    let host = host.trim_matches(['[', ']']).trim();
    !matches!(host, "127.0.0.1" | "::1" | "localhost" | "") && !host.starts_with("127.")
}

async fn security_posture(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<SecurityPosture>, AppError> {
    super::auth::require_admin(&claims)?;

    let (admin_password_change_pending,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM users WHERE must_change_password = TRUE AND is_active = TRUE)",
    )
    .fetch_one(&state.db)
    .await?;

    let owner_rows: Vec<(String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT u.username, u.last_login \
         FROM users u \
         JOIN user_roles ur ON ur.user_id = u.user_id \
         JOIN roles r ON r.role_id = ur.role_id \
         WHERE r.name = 'owner' AND u.is_active = TRUE \
         ORDER BY u.username",
    )
    .fetch_all(&state.db)
    .await?;

    let owners: Vec<SecurityPostureOwner> = owner_rows
        .into_iter()
        .map(|(username, last_login)| SecurityPostureOwner {
            username,
            last_login: last_login.map(|t| t.to_rfc3339()),
        })
        .collect();

    Ok(Json(SecurityPosture {
        listen_addr_public: is_public_bind(&state.listen_addr),
        listen_addr: state.listen_addr.clone(),
        admin_password_change_pending,
        active_owner_count: owners.len() as i64,
        owners,
    }))
}

#[cfg(test)]
mod tests {
    use super::is_public_bind;

    #[test]
    fn only_loopback_counts_as_private() {
        assert!(!is_public_bind("127.0.0.1:3200"));
        assert!(!is_public_bind("[::1]:3200"));
        assert!(!is_public_bind("localhost:3200"));
        assert!(!is_public_bind("127.0.0.53:3200"));

        // The compose default, and the one an operator most needs telling about.
        assert!(is_public_bind("0.0.0.0:3200"));
        assert!(is_public_bind("[::]:3200"));
        assert!(is_public_bind("192.168.1.10:3200"));
    }
}

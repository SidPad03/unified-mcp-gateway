use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use axum::{
    extract::State,
    http::header,
    routing::post,
    Json, Router,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use axum::extract::FromRef;
use crate::{AppError, AppState};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,       // user_id
    pub username: String,
    pub roles: Vec<String>,
    pub exp: usize,
    pub iat: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub must_change_password: bool,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh_token))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user: Option<(Uuid, String, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT user_id, username, password_hash, email, must_change_password FROM users WHERE username = $1 AND is_active = TRUE"
    )
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await?;

    let (user_id, username, password_hash, email, must_change_password) = match user {
        Some(u) => u,
        None => {
            // Burn equivalent Argon2 CPU so an unknown username takes about the
            // same time as a wrong password, closing a user-enumeration timing
            // side channel.
            let salt = SaltString::generate(&mut OsRng);
            let _ = Argon2::default().hash_password(req.password.as_bytes(), &salt);
            return Err(AppError::Unauthorized("Invalid credentials".into()));
        }
    };

    let parsed_hash = PasswordHash::new(&password_hash)
        .map_err(|_| AppError::Internal("Password hash error".into()))?;

    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized("Invalid credentials".into()))?;

    // Update last_login
    sqlx::query("UPDATE users SET last_login = NOW() WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    let roles: Vec<(String,)> = sqlx::query_as(
        "SELECT r.name FROM roles r JOIN user_roles ur ON r.role_id = ur.role_id WHERE ur.user_id = $1"
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let role_names: Vec<String> = roles.into_iter().map(|(n,)| n).collect();

    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.clone(),
        roles: role_names.clone(),
        exp: (now + Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
        application: None,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            user_id: user_id.to_string(),
            username,
            email,
            roles: role_names,
            must_change_password,
        },
    }))
}

async fn refresh_token(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let now = Utc::now();
    let new_claims = Claims {
        sub: claims.sub,
        username: claims.username,
        roles: claims.roles,
        exp: (now + Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
        application: claims.application,
    };

    let token = encode(
        &Header::default(),
        &new_claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )?;

    Ok(Json(serde_json::json!({ "token": token })))
}

/// Whether `(method, path)` is the *only* request a user still flagged
/// `must_change_password` is allowed to make: a `PATCH` to their own user
/// record, to set a new password.
///
/// `path` is the URI path **as seen inside the `/api/v1` nested router**. axum
/// strips the nest prefix from `parts.uri` for handlers/extractors mounted
/// under `.nest("/api/v1", ..)`, so here the path is `/users/{id}` — *not*
/// `/api/v1/users/{id}`. Matching the un-stripped, prefixed path (as an earlier
/// version did) rejects the very request that clears the flag, making the
/// forced first-login password change impossible to complete.
fn is_self_password_change(method: &axum::http::Method, path: &str, user_id: Uuid) -> bool {
    *method == axum::http::Method::PATCH && path == format!("/users/{}", user_id)
}

// Extractor for Claims from JWT or API key
#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for Claims
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing authorization header".into()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("Invalid authorization format".into()))?;

        // API key path: tokens starting with "mcpgw_"
        if token.starts_with("mcpgw_") {
            return resolve_api_key(token, &app_state).await;
        }

        // JWT path
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(app_state.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized("Invalid or expired token".into()))?;

        let claims = token_data.claims;
        let user_id: Uuid = claims
            .sub
            .parse()
            .map_err(|_| AppError::Unauthorized("Invalid token subject".into()))?;

        // Re-validate against the DB on every request so a revoked/deactivated
        // user or a role change takes effect immediately instead of persisting
        // for the token's lifetime (and surviving indefinitely via /refresh).
        let user_row: Option<(bool, bool)> = sqlx::query_as(
            "SELECT is_active, must_change_password FROM users WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&app_state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        let (is_active, must_change_password) =
            user_row.ok_or_else(|| AppError::Unauthorized("User no longer exists".into()))?;
        if !is_active {
            return Err(AppError::Unauthorized("User account is disabled".into()));
        }

        // Load current roles from the DB rather than trusting the ones baked
        // into the token.
        let roles: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM roles r JOIN user_roles ur ON r.role_id = ur.role_id WHERE ur.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&app_state.db)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
        let role_names: Vec<String> = roles.into_iter().map(|(n,)| n).collect();

        // First-login gate: a user still flagged must_change_password may only
        // call their own password-change endpoint until they rotate it. This
        // enforces the gate server-side so it can't be bypassed by ignoring the
        // dashboard screen and calling the API directly.
        if must_change_password
            && !is_self_password_change(&parts.method, parts.uri.path(), user_id)
        {
            return Err(AppError::Forbidden(
                "You must change your password before continuing".into(),
            ));
        }

        Ok(Claims {
            sub: claims.sub,
            username: claims.username,
            roles: role_names,
            exp: claims.exp,
            iat: claims.iat,
            application: claims.application,
        })
    }
}

pub(crate) async fn resolve_api_key(raw_key: &str, state: &AppState) -> Result<Claims, AppError> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let row: Option<(Uuid, Uuid, bool, Option<chrono::DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT key_id, user_id, is_active, expires_at, application FROM api_keys WHERE key_hash = $1",
    )
    .bind(&key_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let (key_id, user_id, is_active, expires_at, application) =
        row.ok_or_else(|| AppError::Unauthorized("Invalid API key".into()))?;

    if !is_active {
        return Err(AppError::Unauthorized("API key is disabled".into()));
    }
    if let Some(exp) = expires_at {
        if exp < Utc::now() {
            return Err(AppError::Unauthorized("API key has expired".into()));
        }
    }

    // Load user
    let user: Option<(String, bool)> = sqlx::query_as(
        "SELECT username, is_active FROM users WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let (username, user_active) =
        user.ok_or_else(|| AppError::Unauthorized("User not found for API key".into()))?;

    if !user_active {
        return Err(AppError::Unauthorized("User account is disabled".into()));
    }

    // Load roles
    let roles: Vec<(String,)> = sqlx::query_as(
        "SELECT r.name FROM roles r JOIN user_roles ur ON r.role_id = ur.role_id WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let role_names: Vec<String> = roles.into_iter().map(|(n,)| n).collect();

    // Update last_used (fire and forget)
    let db = state.db.clone();
    tokio::spawn(async move {
        let _ = sqlx::query("UPDATE api_keys SET last_used = NOW() WHERE key_id = $1")
            .bind(key_id)
            .execute(&db)
            .await;
    });

    let now = Utc::now();
    Ok(Claims {
        sub: user_id.to_string(),
        username,
        roles: role_names,
        exp: (now + Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
        application,
    })
}

pub fn require_admin(claims: &Claims) -> Result<(), AppError> {
    if claims.roles.contains(&"owner".to_string()) {
        Ok(())
    } else {
        Err(AppError::Forbidden("Owner role required".into()))
    }
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("Password hash error: {}", e)))?
        .to_string();
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the login crash reported in GitHub issues #1/#2.
    /// jsonwebtoken 10 defaults to the `aws_lc_rs` backend, which fails at
    /// runtime without a process-wide rustls CryptoProvider the first time a
    /// token is encoded/decoded — so login would 500/502 and the first
    /// authenticated request's token decode would 401. With the `rust_crypto`
    /// backend this HS256 round-trip must succeed with no extra setup.
    #[test]
    fn jwt_hs256_roundtrip_works_without_crypto_provider() {
        let secret = b"test-secret-at-least-16-chars-long";
        let now = Utc::now();
        let claims = Claims {
            sub: "00000000-0000-0000-0000-000000000000".into(),
            username: "admin".into(),
            roles: vec!["owner".into()],
            exp: (now + Duration::hours(1)).timestamp() as usize,
            iat: now.timestamp() as usize,
            application: None,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret),
        )
        .expect("HS256 encode should not require a CryptoProvider");

        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret),
            &Validation::default(),
        )
        .expect("HS256 decode should not require a CryptoProvider");

        assert_eq!(decoded.claims.username, "admin");
        assert_eq!(decoded.claims.roles, vec!["owner".to_string()]);
    }

    /// Regression test for the forced first-login password change dead-end.
    /// The must-change-password gate runs inside the `/api/v1` nested router,
    /// where axum has already stripped the `/api/v1` prefix from the URI. The
    /// gate must therefore match the *stripped* path `/users/{id}` — matching
    /// the prefixed `/api/v1/users/{id}` (as an earlier version did) rejected
    /// the exact PATCH that clears the flag, so the user could never get past
    /// the "Set your password" screen.
    #[test]
    fn must_change_gate_allows_only_self_password_change() {
        use axum::http::Method;
        let me: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
        let other: Uuid = "00000000-0000-0000-0000-000000000009".parse().unwrap();

        // The one request that must be allowed: PATCH my own record, using the
        // path as seen *inside* the /api/v1 nest (prefix already stripped).
        assert!(is_self_password_change(
            &Method::PATCH,
            &format!("/users/{}", me),
            me
        ));

        // Wrong method on my own record — blocked.
        assert!(!is_self_password_change(&Method::GET, &format!("/users/{}", me), me));
        assert!(!is_self_password_change(&Method::DELETE, &format!("/users/{}", me), me));

        // Someone else's record — blocked.
        assert!(!is_self_password_change(
            &Method::PATCH,
            &format!("/users/{}", other),
            me
        ));

        // A different endpoint entirely — blocked.
        assert!(!is_self_password_change(&Method::PATCH, "/policies/abc", me));

        // The un-stripped, /api/v1-prefixed path must NOT match — this is the
        // exact comparison that broke the flow.
        assert!(!is_self_password_change(
            &Method::PATCH,
            &format!("/api/v1/users/{}", me),
            me
        ));
    }
}

use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::{Claims, require_admin, hash_password};
use super::api_keys::generate_app_keys_for_user;

#[derive(Serialize)]
pub struct UserResponse {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub last_login: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub is_active: Option<bool>,
    pub role: Option<String>,
    pub password: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id", patch(update_user).delete(delete_user))
}

async fn list_users(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    require_admin(&claims)?;

    let users: Vec<(Uuid, String, Option<String>, bool, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT user_id, username, email, is_active, created_at, last_login FROM users ORDER BY created_at"
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::new();
    for (user_id, username, email, is_active, created_at, last_login) in users {
        let roles: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM roles r JOIN user_roles ur ON r.role_id = ur.role_id WHERE ur.user_id = $1"
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;

        result.push(UserResponse {
            user_id: user_id.to_string(),
            username,
            email,
            is_active,
            created_at: created_at.to_rfc3339(),
            last_login: last_login.map(|t| t.to_rfc3339()),
            roles: roles.into_iter().map(|(n,)| n).collect(),
        });
    }

    Ok(Json(result))
}

async fn create_user(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    require_admin(&claims)?;

    // A role must be supplied explicitly. Previously an omitted role silently
    // minted a full owner; require and validate it instead so a mistaken or
    // malicious call can't accidentally create an admin (or a roleless user).
    let role_name = req
        .role
        .filter(|r| !r.is_empty())
        .ok_or_else(|| AppError::BadRequest("A role must be specified".into()))?;

    let password_hash = hash_password(&req.password)?;
    let user_id = Uuid::new_v4();

    // Resolve the role up front so we never insert a user we can't assign.
    let (role_id,): (Uuid,) = sqlx::query_as("SELECT role_id FROM roles WHERE name = $1")
        .bind(&role_name)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("Unknown role '{}'", role_name)))?;

    // Insert the user and its role atomically so a failure partway through can
    // never leave a roleless (locked-out) user behind.
    let mut tx = state.db.begin().await?;

    sqlx::query(
        "INSERT INTO users (user_id, username, password_hash, email, is_active)
         VALUES ($1, $2, $3, $4, TRUE)"
    )
    .bind(user_id)
    .bind(&req.username)
    .bind(&password_hash)
    .bind(&req.email)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate") {
            AppError::Conflict("Username already exists".into())
        } else {
            AppError::Internal(e.to_string())
        }
    })?;

    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // Auto-generate per-app API keys
    if let Err(e) = generate_app_keys_for_user(&state.db, user_id).await {
        tracing::warn!("Failed to auto-generate app keys for user {}: {}", user_id, e);
    }

    Ok(Json(UserResponse {
        user_id: user_id.to_string(),
        username: req.username,
        email: req.email,
        is_active: true,
        created_at: chrono::Utc::now().to_rfc3339(),
        last_login: None,
        roles: vec![role_name],
    }))
}

async fn update_user(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let is_self = claims.sub == id.to_string();
    let self_password_only = is_self && req.password.is_some()
        && req.email.is_none() && req.is_active.is_none() && req.role.is_none();
    if !self_password_only {
        require_admin(&claims)?;
    }

    // Guard against demoting or deactivating the last owner, which would lock
    // everyone out of admin functions. Mirrors the check in delete_user.
    let would_remove_owner = matches!(req.is_active, Some(false))
        || req.role.as_deref().map(|r| r != "owner").unwrap_or(false);
    if would_remove_owner {
        let target_is_owner: bool = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM user_roles ur JOIN roles r ON r.role_id = ur.role_id
             WHERE ur.user_id = $1 AND r.name = 'owner'"
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?
        .0 > 0;

        if target_is_owner {
            let owner_count: (i64,) = sqlx::query_as(
                "SELECT COUNT(DISTINCT ur.user_id) FROM user_roles ur
                 JOIN roles r ON r.role_id = ur.role_id
                 WHERE r.name = 'owner'"
            )
            .fetch_one(&state.db)
            .await?;

            if owner_count.0 <= 1 {
                return Err(AppError::BadRequest(
                    "Cannot demote or deactivate the last owner user".into(),
                ));
            }
        }
    }

    if let Some(email) = &req.email {
        sqlx::query("UPDATE users SET email = $1 WHERE user_id = $2")
            .bind(email).bind(id).execute(&state.db).await?;
    }

    if let Some(is_active) = req.is_active {
        sqlx::query("UPDATE users SET is_active = $1 WHERE user_id = $2")
            .bind(is_active).bind(id).execute(&state.db).await?;
    }

    if let Some(password) = &req.password {
        let password_hash = hash_password(password)?;
        // Setting a password clears the first-login "must change password" flag.
        sqlx::query("UPDATE users SET password_hash = $1, must_change_password = FALSE WHERE user_id = $2")
            .bind(&password_hash).bind(id).execute(&state.db).await?;
    }

    if let Some(role_name) = &req.role {
        // Resolve the target role first so an invalid name can't leave the
        // user with no role at all.
        let role: Option<(Uuid,)> = sqlx::query_as(
            "SELECT role_id FROM roles WHERE name = $1"
        )
        .bind(role_name)
        .fetch_optional(&state.db)
        .await?;

        let (role_id,) = role.ok_or_else(|| {
            AppError::BadRequest(format!("Unknown role '{}'", role_name))
        })?;

        // Swap roles atomically.
        let mut tx = state.db.begin().await?;
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
            .bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)")
            .bind(id).bind(role_id).execute(&mut *tx).await?;
        tx.commit().await?;
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

async fn delete_user(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let caller_id: Uuid = claims.sub.parse()
        .map_err(|_| AppError::Internal("Invalid caller ID".into()))?;
    if caller_id == id {
        return Err(AppError::BadRequest("Cannot delete your own account".into()));
    }

    let target: Option<(String,)> = sqlx::query_as(
        "SELECT username FROM users WHERE user_id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    if target.is_none() {
        return Err(AppError::NotFound("User not found".into()));
    }

    // Prevent deleting the last admin
    let target_roles: Vec<(String,)> = sqlx::query_as(
        "SELECT r.name FROM roles r JOIN user_roles ur ON r.role_id = ur.role_id WHERE ur.user_id = $1"
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let is_owner = target_roles.iter().any(|(r,)| r == "owner");
    if is_owner {
        let owner_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(DISTINCT ur.user_id) FROM user_roles ur
             JOIN roles r ON r.role_id = ur.role_id
             WHERE r.name = 'owner'"
        )
        .fetch_one(&state.db)
        .await?;

        if owner_count.0 <= 1 {
            return Err(AppError::BadRequest("Cannot delete the last owner user".into()));
        }
    }

    // CASCADE handles user_roles and api_keys; policies.created_by is SET NULL
    sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

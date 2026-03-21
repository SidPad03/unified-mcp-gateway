use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::{Claims, require_admin};

#[derive(Serialize)]
pub struct RolePolicyInfo {
    pub policy_id: String,
    pub name: String,
    pub tool_pattern: String,
    pub decision: String,
}

#[derive(Serialize)]
pub struct RoleResponse {
    pub role_id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub default_policy: String,
    pub user_count: i64,
    pub policies: Vec<RolePolicyInfo>,
}

#[derive(Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub default_policy: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_policy: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/:id", axum::routing::patch(update_role).delete(delete_role))
        .route("/roles/:id/impact", get(role_impact))
}

async fn list_roles(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<RoleResponse>>, AppError> {
    require_admin(&claims)?;

    let roles: Vec<(Uuid, String, Option<String>, bool, String)> = sqlx::query_as(
        "SELECT role_id, name, description, is_system, default_policy FROM roles ORDER BY name"
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::new();
    for (role_id, name, description, is_system, default_policy) in roles {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM user_roles WHERE role_id = $1"
        )
        .bind(role_id)
        .fetch_one(&state.db)
        .await?;

        // Fetch policies bound to this role
        let policy_rows: Vec<(Uuid, String, String, String)> = sqlx::query_as(
            "SELECT p.policy_id, p.name, p.tool_pattern, p.decision
             FROM policies p
             JOIN role_policies rp ON rp.policy_id = p.policy_id
             WHERE rp.role_id = $1 AND p.is_active = TRUE
             ORDER BY p.priority ASC"
        )
        .bind(role_id)
        .fetch_all(&state.db)
        .await?;

        let policies: Vec<RolePolicyInfo> = policy_rows
            .into_iter()
            .map(|(pid, pname, tool_pattern, decision)| RolePolicyInfo {
                policy_id: pid.to_string(),
                name: pname,
                tool_pattern,
                decision,
            })
            .collect();

        result.push(RoleResponse {
            role_id: role_id.to_string(),
            name,
            description,
            is_system,
            default_policy,
            user_count: count.0,
            policies,
        });
    }

    Ok(Json(result))
}

async fn create_role(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    require_admin(&claims)?;

    let default_policy = match req.default_policy.as_deref() {
        Some("deny") => "deny",
        _ => "allow",
    };

    let role_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO roles (role_id, name, description, permissions, is_system, default_policy)
         VALUES ($1, $2, $3, '[]'::jsonb, FALSE, $4)"
    )
    .bind(role_id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(default_policy)
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate") {
            AppError::Conflict("Role name already exists".into())
        } else {
            AppError::Internal(e.to_string())
        }
    })?;

    Ok(Json(RoleResponse {
        role_id: role_id.to_string(),
        name: req.name,
        description: req.description,
        is_system: false,
        default_policy: default_policy.to_string(),
        user_count: 0,
        policies: vec![],
    }))
}

async fn update_role(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT is_system FROM roles WHERE role_id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    if role.is_none() {
        return Err(AppError::NotFound("Role not found".into()));
    }

    if let Some(name) = &req.name {
        sqlx::query("UPDATE roles SET name = $1 WHERE role_id = $2")
            .bind(name).bind(id).execute(&state.db).await
            .map_err(|e| {
                if e.to_string().contains("duplicate") {
                    AppError::Conflict("Role name already exists".into())
                } else {
                    AppError::Internal(e.to_string())
                }
            })?;
    }
    if let Some(description) = &req.description {
        sqlx::query("UPDATE roles SET description = $1 WHERE role_id = $2")
            .bind(description).bind(id).execute(&state.db).await?;
    }
    if let Some(dp) = &req.default_policy {
        let dp_val = if dp == "deny" { "deny" } else { "allow" };
        sqlx::query("UPDATE roles SET default_policy = $1 WHERE role_id = $2")
            .bind(dp_val).bind(id).execute(&state.db).await?;
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

/// GET /roles/{id}/impact — preview what deleting this role would affect
async fn role_impact(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let role: Option<(String, bool)> = sqlx::query_as(
        "SELECT name, is_system FROM roles WHERE role_id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let (role_name, is_system) = match role {
        None => return Err(AppError::NotFound("Role not found".into())),
        Some(r) => r,
    };

    let affected_users: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT u.user_id, u.username FROM users u
         JOIN user_roles ur ON ur.user_id = u.user_id
         WHERE ur.role_id = $1"
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    // Users who have ONLY this role (will be left with no roles)
    let mut orphaned_users = Vec::new();
    for (uid, uname) in &affected_users {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM user_roles WHERE user_id = $1"
        )
        .bind(uid)
        .fetch_one(&state.db)
        .await?;
        if count.0 <= 1 {
            orphaned_users.push(uname.clone());
        }
    }

    let policy_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM role_policies WHERE role_id = $1"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "role_name": role_name,
        "is_system": is_system,
        "affected_user_count": affected_users.len(),
        "affected_users": affected_users.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
        "orphaned_users": orphaned_users,
        "policy_binding_count": policy_count.0,
    })))
}

async fn delete_role(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT is_system FROM roles WHERE role_id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    match role {
        None => return Err(AppError::NotFound("Role not found".into())),
        Some((true,)) => return Err(AppError::BadRequest("Cannot delete system roles".into())),
        _ => {}
    }

    // Collect impact counts before deletion
    let affected_users: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM user_roles WHERE role_id = $1"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    let policy_bindings: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM role_policies WHERE role_id = $1"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // CASCADE deletes user_roles and role_policies entries
    sqlx::query("DELETE FROM roles WHERE role_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "removed_user_assignments": affected_users.0,
        "removed_policy_bindings": policy_bindings.0,
    })))
}

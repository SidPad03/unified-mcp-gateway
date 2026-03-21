use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::{Claims, require_admin};

#[derive(Serialize)]
pub struct PolicyResponse {
    pub policy_id: String,
    pub name: String,
    pub priority: i32,
    pub tool_pattern: String,
    pub decision: String,
    pub reason: Option<String>,
    pub is_active: bool,
    pub risk_categories: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub role_ids: Vec<String>,
    pub role_names: Vec<String>,
    pub application_match: Option<String>,
}

#[derive(Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub tool_pattern: String,
    pub decision: String,
    pub reason: Option<String>,
    pub role_ids: Option<Vec<Uuid>>,
    pub risk_categories: Option<Vec<String>>,
    pub application_match: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePolicyRequest {
    pub name: Option<String>,
    pub tool_pattern: Option<String>,
    pub priority: Option<i32>,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub is_active: Option<bool>,
    pub role_ids: Option<Vec<Uuid>>,
    pub risk_categories: Option<Vec<String>>,
    pub application_match: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/policies", get(list_policies).post(create_policy))
        .route("/policies/:id", put(update_policy).delete(delete_policy))
}

async fn list_policies(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<PolicyResponse>>, AppError> {
    require_admin(&claims)?;

    let policies: Vec<(Uuid, String, i32, String, String, Option<String>, bool, Option<Vec<String>>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>, Option<String>)> = sqlx::query_as(
        "SELECT policy_id, name, priority, tool_pattern, decision, reason, is_active, risk_categories, created_at, updated_at, application_match
         FROM policies ORDER BY priority ASC"
    )
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::new();
    for (policy_id, name, priority, tool_pattern, decision, reason, is_active, risk_categories, created_at, updated_at, application_match) in policies {
        // Fetch bound roles for this policy
        let role_rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT r.role_id, r.name FROM roles r
             JOIN role_policies rp ON rp.role_id = r.role_id
             WHERE rp.policy_id = $1
             ORDER BY r.name"
        )
        .bind(policy_id)
        .fetch_all(&state.db)
        .await?;

        let role_ids: Vec<String> = role_rows.iter().map(|(id, _)| id.to_string()).collect();
        let role_names: Vec<String> = role_rows.into_iter().map(|(_, name)| name).collect();

        result.push(PolicyResponse {
            policy_id: policy_id.to_string(),
            name,
            priority,
            tool_pattern,
            decision,
            reason,
            is_active,
            risk_categories: risk_categories.unwrap_or_default(),
            created_at: created_at.to_rfc3339(),
            updated_at: updated_at.to_rfc3339(),
            role_ids,
            role_names,
            application_match,
        });
    }

    Ok(Json(result))
}

async fn create_policy(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<Json<PolicyResponse>, AppError> {
    require_admin(&claims)?;

    if !["allow", "deny"].contains(&req.decision.as_str()) {
        return Err(AppError::BadRequest("Decision must be 'allow' or 'deny'".into()));
    }

    let next_priority: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(priority), 0) + 1 FROM policies"
    )
    .fetch_one(&state.db)
    .await?;
    let priority = next_priority.0;

    let policy_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let user_id: Uuid = claims.sub.parse().map_err(|_| AppError::Internal("Invalid user ID".into()))?;

    let risk_cats: Vec<String> = req.risk_categories.clone().unwrap_or_default();

    sqlx::query(
        "INSERT INTO policies (policy_id, name, priority, conditions, decision, reason, is_active, created_by, created_at, updated_at, tool_pattern, risk_categories, application_match)
         VALUES ($1, $2, $3, '{}'::jsonb, $4, $5, TRUE, $6, $7, $7, $8, $9, $10)"
    )
    .bind(policy_id)
    .bind(&req.name)
    .bind(priority)
    .bind(&req.decision)
    .bind(&req.reason)
    .bind(user_id)
    .bind(now)
    .bind(&req.tool_pattern)
    .bind(&risk_cats)
    .bind(&req.application_match)
    .execute(&state.db)
    .await?;

    // Bind roles
    let mut role_ids = Vec::new();
    let mut role_names = Vec::new();
    if let Some(ref ids) = req.role_ids {
        for role_id in ids {
            sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(role_id)
                .bind(policy_id)
                .execute(&state.db)
                .await?;

            let name_row: Option<(String,)> = sqlx::query_as("SELECT name FROM roles WHERE role_id = $1")
                .bind(role_id)
                .fetch_optional(&state.db)
                .await?;
            role_ids.push(role_id.to_string());
            if let Some((name,)) = name_row {
                role_names.push(name);
            }
        }
    }

    Ok(Json(PolicyResponse {
        policy_id: policy_id.to_string(),
        name: req.name,
        priority,
        tool_pattern: req.tool_pattern,
        decision: req.decision,
        reason: req.reason,
        is_active: true,
        risk_categories: risk_cats,
        created_at: now.to_rfc3339(),
        updated_at: now.to_rfc3339(),
        role_ids,
        role_names,
        application_match: req.application_match,
    }))
}

async fn update_policy(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePolicyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    if let Some(ref decision) = req.decision {
        if !["allow", "deny"].contains(&decision.as_str()) {
            return Err(AppError::BadRequest("Decision must be 'allow' or 'deny'".into()));
        }
    }

    if let Some(name) = &req.name {
        sqlx::query("UPDATE policies SET name = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(name).bind(id).execute(&state.db).await?;
    }
    if let Some(priority) = req.priority {
        let conflict: Option<(Uuid,)> = sqlx::query_as(
            "SELECT policy_id FROM policies WHERE priority = $1 AND policy_id != $2"
        )
        .bind(priority).bind(id)
        .fetch_optional(&state.db)
        .await?;
        if conflict.is_some() {
            return Err(AppError::Conflict(format!("Priority {} is already in use", priority)));
        }
        sqlx::query("UPDATE policies SET priority = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(priority).bind(id).execute(&state.db).await?;
    }
    if let Some(tool_pattern) = &req.tool_pattern {
        sqlx::query("UPDATE policies SET tool_pattern = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(tool_pattern).bind(id).execute(&state.db).await?;
    }
    if let Some(decision) = &req.decision {
        sqlx::query("UPDATE policies SET decision = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(decision).bind(id).execute(&state.db).await?;
    }
    if let Some(reason) = &req.reason {
        sqlx::query("UPDATE policies SET reason = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(reason).bind(id).execute(&state.db).await?;
    }
    if let Some(is_active) = req.is_active {
        sqlx::query("UPDATE policies SET is_active = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(is_active).bind(id).execute(&state.db).await?;
    }
    if let Some(ref risk_cats) = req.risk_categories {
        sqlx::query("UPDATE policies SET risk_categories = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(risk_cats).bind(id).execute(&state.db).await?;
    }
    if let Some(ref app_match) = req.application_match {
        sqlx::query("UPDATE policies SET application_match = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(app_match).bind(id).execute(&state.db).await?;
    }

    // Update role bindings if provided
    if let Some(ref role_ids) = req.role_ids {
        // Remove old bindings
        sqlx::query("DELETE FROM role_policies WHERE policy_id = $1")
            .bind(id).execute(&state.db).await?;
        // Insert new bindings
        for role_id in role_ids {
            sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(role_id).bind(id).execute(&state.db).await?;
        }
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

async fn delete_policy(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    sqlx::query("DELETE FROM policies WHERE policy_id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

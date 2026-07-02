use argon2::{Argon2, PasswordHasher};
use argon2::password_hash::SaltString;
use rand::rngs::OsRng;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn seed_defaults(pool: &PgPool) -> Result<(), sqlx::Error> {
    seed_roles(pool).await?;
    seed_admin_user(pool).await?;
    seed_default_policies(pool).await?;
    Ok(())
}

async fn seed_roles(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO roles (role_id, name, description, permissions, is_system, default_policy)
         VALUES ($1, $2, $3, '[]'::jsonb, TRUE, 'allow')
         ON CONFLICT (name) DO NOTHING"
    )
    .bind(Uuid::new_v4())
    .bind("owner")
    .bind("Full access owner with all permissions")
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_admin_user(pool: &PgPool) -> Result<(), sqlx::Error> {
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT username FROM users WHERE username = 'admin'"
    )
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Ok(());
    }

    // Default to the well-known admin/admin so a fresh install is easy to log
    // into, but flag must_change_password so the server forces a rotation on
    // first login (enforced in the JWT extractor) — the default is never a
    // lasting credential. If the operator presets MCPGW_ADMIN_PASSWORD they've
    // chosen deliberately, so we don't force a change in that case.
    let (password, force_change) = match std::env::var("MCPGW_ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => (p, false),
        _ => ("admin".to_string(), true),
    };

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Failed to hash password")
        .to_string();

    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (user_id, username, password_hash, email, is_active, must_change_password)
         VALUES ($1, 'admin', $2, 'admin@mcp-gateway.local', TRUE, $3)"
    )
    .bind(user_id)
    .bind(&password_hash)
    .bind(force_change)
    .execute(pool)
    .await?;

    let owner_role: Option<(Uuid,)> = sqlx::query_as(
        "SELECT role_id FROM roles WHERE name = 'owner'"
    )
    .fetch_optional(pool)
    .await?;

    if let Some((role_id,)) = owner_role {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING"
        )
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await?;
    }

    // Auto-generate per-app API keys for admin
    if let Err(e) = crate::api::api_keys::generate_app_keys_for_user(pool, user_id).await {
        tracing::warn!("Failed to auto-generate app keys for admin: {}", e);
    }

    if force_change {
        tracing::warn!(
            "Created default admin user (username: admin, password: admin). \
             You will be required to change this password on first login."
        );
    } else {
        tracing::info!("Created default admin user (username: admin) using MCPGW_ADMIN_PASSWORD.");
    }
    Ok(())
}

async fn seed_default_policies(pool: &PgPool) -> Result<(), sqlx::Error> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM policies"
    )
    .fetch_optional(pool)
    .await?;

    if existing.map(|(c,)| c).unwrap_or(0) > 0 {
        return Ok(());
    }

    let owner_role: Option<(Uuid,)> = sqlx::query_as(
        "SELECT role_id FROM roles WHERE name = 'owner'"
    )
    .fetch_optional(pool)
    .await?;

    let owner_id = owner_role.map(|(id,)| id);

    // Policy 1: Deny destructive operations. This must have a lower priority
    // number than the broad allow rule below: the engine sorts ascending by
    // priority and returns the first match, so a specific deny has to be
    // evaluated before "allow *" or it would never fire.
    let deny_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO policies (policy_id, name, priority, conditions, decision, reason, is_active, tool_pattern)
         VALUES ($1, $2, $3, '{}'::jsonb, $4, $5, TRUE, $6)"
    )
    .bind(deny_id)
    .bind("Deny destructive operations")
    .bind(1)
    .bind("deny")
    .bind("Block destructive operations like drop or delete")
    .bind("*drop_*")
    .execute(pool)
    .await?;

    if let Some(role_id) = owner_id {
        sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(role_id).bind(deny_id).execute(pool).await?;
    }

    // Policy 2: Allow everything else for owner (catch-all after the deny).
    let allow_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO policies (policy_id, name, priority, conditions, decision, reason, is_active, tool_pattern)
         VALUES ($1, $2, $3, '{}'::jsonb, $4, $5, TRUE, $6)"
    )
    .bind(allow_id)
    .bind("Allow all tools")
    .bind(2)
    .bind("allow")
    .bind("Grant full tool access")
    .bind("*")
    .execute(pool)
    .await?;

    if let Some(role_id) = owner_id {
        sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(role_id).bind(allow_id).execute(pool).await?;
    }

    tracing::info!("Seeded default policies (allow all + deny destructive)");
    Ok(())
}

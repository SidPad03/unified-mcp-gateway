use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use rand::rngs::OsRng;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn seed_defaults(pool: &PgPool) -> Result<(), sqlx::Error> {
    seed_roles(pool).await?;
    seed_admin_user(pool).await?;
    seed_default_policies(pool).await?;
    // Ensure existing users have keys for any application added after they were
    // created (e.g. clawbot / codex), so those clients appear in the connect
    // list and on the usage graph (whose app nodes come from `api_keys`).
    // Idempotent, and it has to run on every startup — it used to sit at the
    // end of `seed_default_policies`, which returns early as soon as any policy
    // exists, so it only ever ran on a brand-new database where the sole user
    // was the admin who already had keys.
    crate::api::api_keys::backfill_app_keys_for_all_users(pool).await?;
    Ok(())
}

async fn seed_roles(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO roles (role_id, name, description, permissions, is_system, default_policy)
         VALUES ($1, $2, $3, '[]'::jsonb, TRUE, 'allow')
         ON CONFLICT (name) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind("owner")
    .bind("Full access owner with all permissions")
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_admin_user(pool: &PgPool) -> Result<(), sqlx::Error> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT username FROM users WHERE username = 'admin'")
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

    let owner_role: Option<(Uuid,)> =
        sqlx::query_as("SELECT role_id FROM roles WHERE name = 'owner'")
            .fetch_optional(pool)
            .await?;

    if let Some((role_id,)) = owner_role {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
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
    let existing: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM policies")
        .fetch_optional(pool)
        .await?;

    if existing.map(|(c,)| c).unwrap_or(0) > 0 {
        return Ok(());
    }

    let owner_role: Option<(Uuid,)> =
        sqlx::query_as("SELECT role_id FROM roles WHERE name = 'owner'")
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
        sqlx::query(
            "INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(deny_id)
        .execute(pool)
        .await?;
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
        sqlx::query(
            "INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(allow_id)
        .execute(pool)
        .await?;
    }

    tracing::info!("Seeded default policies (allow all + deny destructive)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::api_keys::SUPPORTED_APPS;
    use crate::test_support::lock_env_async;

    /// A migrated, empty scratch database. Returns `None` when
    /// `TEST_DATABASE_URL` is unset so `cargo test` still works with no
    /// database, matching the config-transfer tests.
    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("TEST_DATABASE_URL is set but unreachable");
        crate::db::run_migrations(&pool)
            .await
            .expect("migrations should apply");
        // Children before parents so the foreign keys hold at every step.
        for table in [
            "audit_events",
            "api_keys",
            "tool_registry",
            "backends",
            "role_policies",
            "policies",
            "user_roles",
            "users",
            "roles",
        ] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        Some(pool)
    }

    /// Regression test for a backfill that never ran.
    ///
    /// `backfill_app_keys_for_all_users` exists so a user created before an
    /// application was added to `SUPPORTED_APPS` still gets a key for it —
    /// without one, that client never appears in the connect list or on the
    /// usage graph. It used to be called from the end of
    /// `seed_default_policies`, which returns early as soon as the database
    /// has any policy. So it ran only on a brand-new deployment, where the
    /// only user is the admin who was just given keys anyway, and never on the
    /// established deployments it was written for.
    #[tokio::test]
    async fn backfill_reaches_users_on_a_database_that_already_has_policies() {
        let _guard = lock_env_async().await;
        std::env::set_var("JWT_SECRET", "seed-test-secret-at-least-16-chars");
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };

        // A user predating the current app list, holding no keys at all.
        let user_id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (user_id, username, password_hash, email, is_active)
             VALUES ($1, 'legacy-user', '$argon2id$fixture', 'legacy@example.com', TRUE)",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        // A policy already present, which is what trips the early return.
        sqlx::query(
            "INSERT INTO policies (policy_id, name, priority, conditions, decision, is_active, tool_pattern)
             VALUES ($1, 'pre-existing policy', 999, '{}'::jsonb, 'allow', TRUE, '*')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        let (before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, 0, "the fixture user should start with no keys");

        seed_defaults(&pool).await.expect("seeding should succeed");

        let (after,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            after,
            SUPPORTED_APPS.len() as i64,
            "every supported application should have been backfilled for the pre-existing user"
        );

        // Idempotent: a second startup must not duplicate or rotate anything.
        seed_defaults(&pool)
            .await
            .expect("second seed should succeed");
        let (twice,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(twice, after, "re-running the backfill must be a no-op");
    }
}

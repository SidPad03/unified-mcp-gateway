pub mod seed;

use sqlx::{Executor, PgPool};

/// All database schema migrations, applied in order on startup and recorded in
/// `schema_migrations` so each one runs exactly once.
///
/// The DDL is written with IF NOT EXISTS guards and is safe to re-run, but
/// several blocks also carry one-shot DML — 006 resets the owner role's default
/// policy, 012 deletes the audit rows internal calls left behind — and DML has
/// no such guard. Re-running those every boot silently reverted operator
/// configuration and re-deleted rows. 010 solved it once, by hand, with an
/// information_schema check; `run_migrations` now solves it for all of them.
///
/// Never edit a released entry: an upgraded database has already recorded it
/// and will not run it again. Append a new one instead.
const MIGRATIONS: &[&str] = &[
    // 001: Core tables — users, roles, policies, audit, backends, tools
    r#"
CREATE TABLE IF NOT EXISTS users (
    user_id UUID PRIMARY KEY,
    username VARCHAR(255) UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    email VARCHAR(255),
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS roles (
    role_id UUID PRIMARY KEY,
    name VARCHAR(255) UNIQUE NOT NULL,
    description TEXT,
    permissions JSONB NOT NULL DEFAULT '[]',
    is_system BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_roles (
    user_id UUID REFERENCES users(user_id) ON DELETE CASCADE,
    role_id UUID REFERENCES roles(role_id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

CREATE TABLE IF NOT EXISTS policies (
    policy_id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    priority INTEGER NOT NULL,
    conditions JSONB NOT NULL,
    decision VARCHAR(50) NOT NULL,
    reason TEXT,
    notify BOOLEAN DEFAULT FALSE,
    is_active BOOLEAN DEFAULT TRUE,
    created_by UUID REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS audit_events (
    event_id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    trace_id UUID NOT NULL,
    session_id VARCHAR(255),
    user_id UUID,
    client_id VARCHAR(255),
    tool_name VARCHAR(512) NOT NULL,
    backend_name VARCHAR(255) NOT NULL,
    risk_category VARCHAR(100),
    request_hash VARCHAR(64),
    request_payload TEXT,
    response_hash VARCHAR(64),
    response_payload TEXT,
    duration_ms DOUBLE PRECISION,
    status VARCHAR(50) NOT NULL,
    error_message TEXT,
    policy_decision VARCHAR(50),
    policy_id UUID,
    risk_flags JSONB DEFAULT '[]',
    metadata JSONB DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_tool ON audit_events(tool_name);
CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_events(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_status ON audit_events(status);
CREATE INDEX IF NOT EXISTS idx_audit_backend ON audit_events(backend_name);

CREATE TABLE IF NOT EXISTS backends (
    backend_id UUID PRIMARY KEY,
    name VARCHAR(255) UNIQUE NOT NULL,
    transport VARCHAR(50) NOT NULL,
    config JSONB NOT NULL,
    risk_category VARCHAR(100),
    is_enabled BOOLEAN DEFAULT TRUE,
    health_status VARCHAR(50) DEFAULT 'idle',
    last_health_check TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS tool_registry (
    tool_id UUID PRIMARY KEY,
    tool_name VARCHAR(512) NOT NULL,
    backend_id UUID REFERENCES backends(backend_id) ON DELETE CASCADE,
    original_name VARCHAR(512) NOT NULL,
    description TEXT,
    input_schema JSONB,
    risk_category VARCHAR(100),
    is_enabled BOOLEAN DEFAULT TRUE,
    last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tools_name ON tool_registry(tool_name);
CREATE INDEX IF NOT EXISTS idx_tools_backend ON tool_registry(backend_id);
"#,
    // 002: API keys
    r#"
CREATE TABLE IF NOT EXISTS api_keys (
    key_id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    key_hash VARCHAR(64) NOT NULL,
    key_prefix VARCHAR(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
"#,
    // 003: Policy redesign — tool_pattern column, role_policies join table
    r#"
ALTER TABLE policies ADD COLUMN IF NOT EXISTS tool_pattern VARCHAR(512) DEFAULT '*';

CREATE TABLE IF NOT EXISTS role_policies (
    role_id UUID REFERENCES roles(role_id) ON DELETE CASCADE,
    policy_id UUID REFERENCES policies(policy_id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, policy_id)
);
"#,
    // 004: Fix referential integrity — policies.created_by ON DELETE SET NULL
    r#"
ALTER TABLE policies DROP CONSTRAINT IF EXISTS policies_created_by_fkey;
ALTER TABLE policies ADD CONSTRAINT policies_created_by_fkey
  FOREIGN KEY (created_by) REFERENCES users(user_id) ON DELETE SET NULL;
"#,
    // 005: Unique policy priorities
    r#"
DO $$
DECLARE
  rec RECORD;
  next_prio INTEGER;
BEGIN
  FOR rec IN
    SELECT policy_id, priority,
           ROW_NUMBER() OVER (PARTITION BY priority ORDER BY created_at) AS rn
    FROM policies
  LOOP
    IF rec.rn > 1 THEN
      SELECT COALESCE(MAX(priority), 0) + 1 INTO next_prio FROM policies;
      UPDATE policies SET priority = next_prio WHERE policy_id = rec.policy_id;
    END IF;
  END LOOP;
END $$;

DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'policies_priority_unique') THEN
    ALTER TABLE policies ADD CONSTRAINT policies_priority_unique UNIQUE (priority);
  END IF;
END $$;

ALTER TABLE backends ALTER COLUMN health_status SET DEFAULT 'idle';
UPDATE backends SET health_status = 'idle' WHERE health_status = 'unknown';
"#,
    // 006: Role default policy (allow/deny fallback per role)
    r#"
ALTER TABLE roles ADD COLUMN IF NOT EXISTS default_policy VARCHAR(10) NOT NULL DEFAULT 'allow';
UPDATE roles SET default_policy = 'allow' WHERE name = 'owner';
DELETE FROM policies WHERE name = 'Allow all tools (engineering)';
"#,
    // 007: Risk categories on policies
    r#"
ALTER TABLE policies ADD COLUMN IF NOT EXISTS risk_categories TEXT[] DEFAULT NULL;
"#,
    // 008: Application identity — per-app API keys and audit tracking
    r#"
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS application VARCHAR(64);
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_user_app
  ON api_keys(user_id, application) WHERE application IS NOT NULL;

ALTER TABLE policies ADD COLUMN IF NOT EXISTS application_match VARCHAR(512);

ALTER TABLE audit_events ADD COLUMN IF NOT EXISTS application VARCHAR(64);
CREATE INDEX IF NOT EXISTS idx_audit_application ON audit_events(application) WHERE application IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_audit_user_app_time ON audit_events(user_id, application, timestamp DESC);
"#,
    // 009: Tool registry upsert — unique constraint on (backend_id, original_name)
    r#"
DELETE FROM tool_registry a
USING tool_registry b
WHERE a.backend_id = b.backend_id
  AND a.original_name = b.original_name
  AND a.tool_id != b.tool_id
  AND (a.last_seen < b.last_seen OR (a.last_seen = b.last_seen AND a.tool_id < b.tool_id));

CREATE UNIQUE INDEX IF NOT EXISTS idx_tools_backend_original
    ON tool_registry(backend_id, original_name);
"#,
    // 010: Force a password change on first login. Adds a per-user flag; the
    // default `admin` account is flagged once so the operator must set a real
    // password before using the dashboard. The UPDATE is guarded so it only
    // runs the first time the column is added — migrations execute on every
    // startup, and an unconditional UPDATE would re-flag admin after they had
    // already chosen a new password.
    r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'users' AND column_name = 'must_change_password'
    ) THEN
        ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT FALSE;
        UPDATE users SET must_change_password = TRUE WHERE username = 'admin';
    END IF;
END $$;
"#,
    // 011: Store an encrypted copy of each API key so the dashboard can reveal
    // it for the "copy client config" flow. Encrypted at rest with
    // ChaCha20-Poly1305 under a JWT_SECRET-derived key — a DB-only leak can't
    // recover the keys. NULL for keys created before this column (those get
    // rotated on first reveal).
    r#"
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS key_secret TEXT;
"#,
    // 012: Internal tools, and a place to keep gateway settings.
    //
    // `is_internal` marks the gateway's own plumbing — the control tools an
    // agent registers so the gateway can configure it. They are still routed
    // and still policed; they are simply not part of the inventory the
    // operator put behind the gateway, so they stay off the Tools page, out of
    // the counts and out of the audit trail. The backfill names them
    // explicitly rather than matching a prefix, because a backend may
    // legitimately ship a tool whose name begins the same way; the list is
    // `backends::classifier::control_tool_names`, and the two must agree.
    r#"
ALTER TABLE tool_registry ADD COLUMN IF NOT EXISTS is_internal BOOLEAN NOT NULL DEFAULT FALSE;

UPDATE tool_registry SET is_internal = TRUE WHERE original_name IN (
    'agent_list_local_servers',
    'agent_get_local_server_status',
    'agent_get_local_server_logs',
    'agent_install_mcp_server',
    'agent_update_config',
    'agent_remove_mcp_server',
    'agent_start_local_server',
    'agent_stop_local_server',
    'agent_restart_local_server'
);

-- Almost every read wants the visible tools, so the index carries only those.
CREATE INDEX IF NOT EXISTS idx_tools_visible ON tool_registry(backend_id) WHERE is_internal = FALSE;

-- Audit rows written before internal calls stopped being recorded. The trail is
-- meant to be about the operator's own traffic; leaving these in would put a
-- burst of gateway_* and agent_* calls in the middle of it.
DELETE FROM audit_events
WHERE backend_name = 'gateway'
   OR tool_name LIKE '%\_\_agent\_list\_local\_servers'
   OR tool_name LIKE '%\_\_agent\_get\_local\_server\_status'
   OR tool_name LIKE '%\_\_agent\_get\_local\_server\_logs'
   OR tool_name LIKE '%\_\_agent\_install\_mcp\_server'
   OR tool_name LIKE '%\_\_agent\_update\_config'
   OR tool_name LIKE '%\_\_agent\_remove\_mcp\_server'
   OR tool_name LIKE '%\_\_agent\_start\_local\_server'
   OR tool_name LIKE '%\_\_agent\_stop\_local\_server'
   OR tool_name LIKE '%\_\_agent\_restart\_local\_server';

CREATE TABLE IF NOT EXISTS settings (
    key VARCHAR(64) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
"#,
];

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    // The ledger has to exist before anything can be recorded in it, and it is
    // created outside the MIGRATIONS array so it cannot itself need recording.
    pool.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version    INTEGER PRIMARY KEY,
             applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
         )",
    )
    .await?;

    let applied: Vec<(i32,)> = sqlx::query_as("SELECT version FROM schema_migrations")
        .fetch_all(pool)
        .await?;
    let applied: std::collections::HashSet<i32> = applied.into_iter().map(|(v,)| v).collect();

    for (index, sql) in MIGRATIONS.iter().enumerate() {
        // 1-based so the version matches the `// 001:` comments above.
        let version = index as i32 + 1;
        if applied.contains(&version) {
            continue;
        }
        // A database upgraded from a build without this table has an empty
        // ledger, so every block runs one final time — exactly what happened on
        // every boot before — and is then recorded.
        pool.execute(*sql).await?;
        sqlx::query("INSERT INTO schema_migrations (version) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(version)
            .execute(pool)
            .await?;
        tracing::info!(version, "Applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;
    use crate::backends::classifier::control_tool_names;

    /// Migration 012 backfills `is_internal` from a list of names written out
    /// in SQL, and `register_discovered_tools` sets the same flag from
    /// `classifier::is_control_tool`. Two lists, one meaning: if a control tool
    /// is added to the classifier and not to the migration, every row already
    /// in an upgraded database stays visible on the Tools page until that
    /// agent happens to reconnect.
    /// Migration 006 resets the owner role's default policy and 012 deletes the
    /// audit rows internal calls left behind. Both are one-shot DML written
    /// inside a block that used to be executed on every process start, so an
    /// operator who set the owner default to `deny` found it back at `allow`
    /// after the next restart, and a backend whose calls were filed under the
    /// deleted names lost that history every time.
    #[tokio::test]
    async fn a_migration_runs_once_and_not_again_on_the_next_start() {
        let _guard = crate::test_support::lock_db().await;
        let Some(url) = std::env::var("TEST_DATABASE_URL").ok() else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("TEST_DATABASE_URL is set but unreachable");

        super::run_migrations(&pool).await.unwrap();

        // The `owner` role is seeded, not migrated, so the test provides it.
        sqlx::query(
            "INSERT INTO roles (role_id, name, permissions, is_system, default_policy) \
             VALUES ($1, 'owner', '[]'::jsonb, TRUE, 'allow') ON CONFLICT (name) DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        // What an operator can change through the Roles API, and what migration
        // 006 would put back.
        sqlx::query("UPDATE roles SET default_policy = 'deny' WHERE name = 'owner'")
            .execute(&pool)
            .await
            .unwrap();

        // A row filed under a name migration 012 deletes.
        let event_id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO audit_events (event_id, trace_id, tool_name, backend_name, status) \
             VALUES ($1, $2, 'gateway_get_health', 'gateway', 'success')",
        )
        .bind(event_id)
        .bind(uuid::Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();

        // The next boot.
        super::run_migrations(&pool).await.unwrap();

        let (default_policy,): (String,) =
            sqlx::query_as("SELECT default_policy FROM roles WHERE name = 'owner'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            default_policy, "deny",
            "migration 006 must not re-run and revert the operator's choice"
        );

        let (still_there,): (bool,) =
            sqlx::query_as("SELECT EXISTS (SELECT 1 FROM audit_events WHERE event_id = $1)")
                .bind(event_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            still_there,
            "migration 012 must not re-run and delete audit rows on every start"
        );

        sqlx::query("DELETE FROM audit_events WHERE event_id = $1")
            .bind(event_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE roles SET default_policy = 'allow' WHERE name = 'owner'")
            .execute(&pool)
            .await
            .unwrap();
    }

    #[test]
    fn the_backfill_names_every_control_tool() {
        let migration = MIGRATIONS
            .iter()
            .find(|sql| sql.contains("is_internal"))
            .expect("migration 012 is present");

        for name in control_tool_names() {
            assert!(
                migration.contains(&format!("'{name}'")),
                "{name} is a control tool but migration 012 does not backfill it"
            );
            // 012 has *two* lists: the UPDATE that sets the flag, and the
            // DELETE that clears the rows an upgraded trail already holds. The
            // second is written as escaped LIKE patterns, so a name added to
            // the first and not the second leaves those rows in the trail.
            let escaped = name.replace('_', "\\_");
            assert!(
                migration.contains(&escaped),
                "{name} is backfilled by migration 012 but its audit rows are not cleared"
            );
        }
    }

    /// The other direction. A name dropped from the classifier but left in the
    /// migration would keep deleting an operator's audit rows for a tool the
    /// gateway no longer considers its own.
    #[test]
    fn the_backfill_names_nothing_the_classifier_does_not() {
        let migration = MIGRATIONS
            .iter()
            .find(|sql| sql.contains("is_internal"))
            .expect("migration 012 is present");

        let known: Vec<&str> = control_tool_names().to_vec();
        for line in migration.lines() {
            let line = line.trim();
            let Some(name) = line
                .strip_prefix('\'')
                .and_then(|rest| rest.split('\'').next())
            else {
                continue;
            };
            if !name.starts_with("agent_") {
                continue;
            }
            assert!(
                known.contains(&name),
                "migration 012 backfills '{name}', which the classifier does not know"
            );
        }
    }
}

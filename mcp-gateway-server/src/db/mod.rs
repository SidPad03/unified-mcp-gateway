pub mod seed;

use sqlx::{Executor, PgPool};

/// All database schema migrations, applied idempotently on startup.
/// Each statement uses IF NOT EXISTS / IF NOT EXISTS guards so they
/// are safe to re-run on an already-migrated database.
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
];

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    for sql in MIGRATIONS {
        pool.execute(*sql).await?;
    }
    Ok(())
}

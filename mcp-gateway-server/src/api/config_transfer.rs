//! Whole-deployment configuration export and import.
//!
//! Produces a single encrypted bundle containing every row of every table —
//! users, roles, policies, backends, tools, API keys, and (optionally) the audit
//! log — so a gateway can be reproduced 1:1 on different hardware.
//!
//! # Why the bundle is encrypted rather than plain JSON
//!
//! Two independent reasons:
//!
//! 1. **It is a credential dump.** It carries Argon2 password hashes, API key
//!    hashes, and backend configs that routinely hold bearer tokens. A plaintext
//!    file would be the single most dangerous artifact this product can emit.
//! 2. **API keys would not survive the move.** `api_keys.key_secret` is
//!    encrypted at rest under `SHA-256(JWT_SECRET)` (see [`super::api_keys`]).
//!    A target deployment has a *different* `JWT_SECRET`, so a copied blob is
//!    undecryptable there and the dashboard's "reveal key" flow would break.
//!
//! So export decrypts each key with the source's secret, seals the whole payload
//! under a passphrase-derived key, and import re-encrypts each key under the
//! *target's* `JWT_SECRET`. Authentication itself would survive either way —
//! `key_hash` is a plain SHA-256 of the key and is host-independent — but the
//! reveal capability only survives this round trip.
//!
//! # Format
//!
//! `gzip(JSON payload)` sealed with ChaCha20-Poly1305, keyed by Argon2id over the
//! operator's passphrase. The envelope stores the KDF parameters so a bundle
//! stays readable if the defaults are retuned later.
//!
//! # Schema drift
//!
//! Rows move as JSON objects produced by `row_to_json` and are re-inserted with
//! `jsonb_populate_record`, which maps a JSON object onto the *target's* row type.
//! Postgres does the column matching and type coercion: keys the target doesn't
//! have are ignored, columns the bundle lacks land as NULL. That keeps a bundle
//! importable across versions that added or dropped a column, and — because no
//! identifier from the bundle is ever interpolated into SQL — leaves no room for
//! injection from a crafted file.

use axum::{extract::State, routing::post, Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

use super::auth::{require_admin, Claims};
use crate::{AppError, AppState};

/// Bundles are only accepted if they declare this format.
const FORMAT: &str = "mcp-gateway-config-export";
/// Bumped only for a change that older servers cannot read.
const FORMAT_VERSION: u32 = 1;

/// A short passphrase would make the Argon2id work factor irrelevant.
const MIN_PASSPHRASE_LEN: usize = 12;

/// Argon2id parameters (OWASP's recommended baseline). Stored in the envelope so
/// retuning these later cannot orphan bundles produced today.
const KDF_M_COST: u32 = 19_456; // 19 MiB
const KDF_T_COST: u32 = 2;
const KDF_P_COST: u32 = 1;

/// Tables in dependency order — parents before children. Import inserts in this
/// order and clears in reverse, so foreign keys hold at every step.
const TABLES: &[&str] = &[
    "roles",
    "users",
    "user_roles",
    "policies",
    "role_policies",
    "backends",
    "tool_registry",
    "api_keys",
];

/// Exported separately because it is opt-out: it is usually the bulk of a bundle.
const AUDIT_TABLE: &str = "audit_events";

/// A whole deployment's audit history does not fit in the global 8 MiB request
/// cap, so these two routes carry a much larger one of their own rather than
/// widening the limit for every endpoint. Both are owner-only.
const CONFIG_TRANSFER_BODY_LIMIT: usize = 512 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config/export", post(export_config))
        .route("/config/import", post(import_config))
        .layer(axum::extract::DefaultBodyLimit::max(
            CONFIG_TRANSFER_BODY_LIMIT,
        ))
}

// ── Wire types ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExportRequest {
    pub passphrase: String,
    /// Include the audit log. Default true — a 1:1 clone includes history.
    #[serde(default = "default_true")]
    pub include_audit: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    pub salt: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

/// The file an operator downloads. Everything outside `ciphertext` is metadata
/// needed to decrypt; no configuration data leaks through it.
#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub format_version: u32,
    pub created_at: String,
    pub source_version: String,
    pub includes_audit: bool,
    pub kdf: KdfParams,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Deserialize)]
pub struct ImportRequest {
    pub passphrase: String,
    pub bundle: Bundle,
}

#[derive(Serialize)]
pub struct ImportSummary {
    pub imported: Vec<TableCount>,
    pub source_version: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct TableCount {
    pub table: String,
    pub rows: i64,
}

/// Decrypted contents: table name → rows as JSON objects.
#[derive(Serialize, Deserialize, Default)]
struct Payload {
    tables: std::collections::BTreeMap<String, Vec<serde_json::Value>>,
}

// ── Crypto ──────────────────────────────────────────────────────────────

/// Derive the bundle key from the operator's passphrase.
fn derive_key(passphrase: &str, salt: &[u8], p: &KdfParams) -> Result<[u8; 32], AppError> {
    if p.algorithm != "argon2id" {
        return Err(AppError::BadRequest(format!(
            "Unsupported key derivation '{}'",
            p.algorithm
        )));
    }
    // Reject absurd parameters from a hostile bundle before allocating for them.
    if p.m_cost > 1_048_576 || p.t_cost > 16 || p.p_cost > 16 {
        return Err(AppError::BadRequest(
            "Bundle declares unreasonable key-derivation parameters".into(),
        ));
    }
    let params = argon2::Params::new(p.m_cost, p.t_cost, p.p_cost, Some(32))
        .map_err(|e| AppError::BadRequest(format!("Invalid key-derivation parameters: {e}")))?;
    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|_| AppError::Internal("Key derivation failed".into()))?;
    Ok(key)
}

fn seal(plaintext: &[u8], passphrase: &str) -> Result<(KdfParams, String, String), AppError> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let params = KdfParams {
        algorithm: "argon2id".into(),
        salt: B64.encode(salt),
        m_cost: KDF_M_COST,
        t_cost: KDF_T_COST,
        p_cost: KDF_P_COST,
    };
    let key = derive_key(passphrase, &salt, &params)?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = ChaCha20Poly1305::new(Key::from_slice(&key))
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| AppError::Internal("Failed to encrypt export bundle".into()))?;

    Ok((params, B64.encode(nonce_bytes), B64.encode(ciphertext)))
}

fn unseal(bundle: &Bundle, passphrase: &str) -> Result<Vec<u8>, AppError> {
    let salt = B64
        .decode(&bundle.kdf.salt)
        .map_err(|_| AppError::BadRequest("Bundle salt is not valid base64".into()))?;
    let nonce = B64
        .decode(&bundle.nonce)
        .map_err(|_| AppError::BadRequest("Bundle nonce is not valid base64".into()))?;
    if nonce.len() != 12 {
        return Err(AppError::BadRequest(
            "Bundle nonce has the wrong length".into(),
        ));
    }
    let ciphertext = B64
        .decode(&bundle.ciphertext)
        .map_err(|_| AppError::BadRequest("Bundle ciphertext is not valid base64".into()))?;

    let key = derive_key(passphrase, &salt, &bundle.kdf)?;
    // AEAD failure here is overwhelmingly a wrong passphrase; it is also what a
    // tampered bundle looks like. Say so without distinguishing the two.
    ChaCha20Poly1305::new(Key::from_slice(&key))
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            AppError::BadRequest(
                "Could not decrypt the bundle — wrong passphrase, or the file is corrupt.".into(),
            )
        })
}

fn gzip(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)
        .and_then(|_| enc.finish())
        .map_err(|e| AppError::Internal(format!("Failed to compress bundle: {e}")))
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|_| AppError::BadRequest("Bundle payload is not valid gzip data".into()))?;
    Ok(out)
}

// ── Export ──────────────────────────────────────────────────────────────

async fn export_config(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<ExportRequest>,
) -> Result<Json<Bundle>, AppError> {
    require_admin(&claims)?;
    if req.passphrase.chars().count() < MIN_PASSPHRASE_LEN {
        return Err(AppError::BadRequest(format!(
            "Passphrase must be at least {MIN_PASSPHRASE_LEN} characters"
        )));
    }

    let payload = collect_payload(&state.db, req.include_audit).await?;

    let json = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Internal(format!("Failed to serialize bundle: {e}")))?;
    let (kdf, nonce, ciphertext) = seal(&gzip(&json)?, &req.passphrase)?;

    let row_total: usize = payload.tables.values().map(|v| v.len()).sum();
    tracing::warn!(
        actor = %claims.sub,
        rows = row_total,
        include_audit = req.include_audit,
        "Configuration export produced — bundle contains credential material"
    );
    record_transfer(&state, &claims, "config.export", row_total as i64).await;

    Ok(Json(Bundle {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        includes_audit: req.include_audit,
        kdf,
        nonce,
        ciphertext,
    }))
}

/// Read every exportable table into a payload.
///
/// Split out from the handler so the round trip can be tested against a real
/// database without standing up an `AppState` and an HTTP stack.
async fn collect_payload(pool: &sqlx::PgPool, include_audit: bool) -> Result<Payload, AppError> {
    let mut payload = Payload::default();
    let mut wanted: Vec<&str> = TABLES.to_vec();
    if include_audit {
        wanted.push(AUDIT_TABLE);
    }

    for table in wanted {
        // `table` is one of our own compile-time constants, never user input.
        let sql = format!("SELECT row_to_json(t) FROM {table} t");
        let mut rows: Vec<serde_json::Value> = sqlx::query_scalar(&sql).fetch_all(pool).await?;

        if table == "api_keys" {
            for row in rows.iter_mut() {
                rekey_for_export(row);
            }
        }
        payload.tables.insert(table.to_string(), rows);
    }
    Ok(payload)
}

/// Replace every exportable table's contents with the payload, in one
/// transaction. Counterpart to [`collect_payload`].
async fn restore_payload(
    pool: &sqlx::PgPool,
    payload: &Payload,
) -> Result<Vec<TableCount>, AppError> {
    // Everything below is one transaction: a failure part-way leaves the
    // deployment exactly as it was rather than half-wiped.
    let mut tx = pool.begin().await?;

    let mut ordered: Vec<&str> = TABLES.to_vec();
    if payload.tables.contains_key(AUDIT_TABLE) {
        ordered.push(AUDIT_TABLE);
    }

    // Replace mode: clear children before parents.
    for table in ordered.iter().rev() {
        sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&mut *tx)
            .await?;
    }

    let mut imported = Vec::new();
    for table in &ordered {
        let Some(rows) = payload.tables.get(*table) else {
            continue;
        };
        let mut count = 0i64;
        for row in rows {
            let mut row = row.clone();
            if *table == "api_keys" {
                rekey_for_import(&mut row)?;
            }
            // jsonb_populate_record maps the object onto the target's row type,
            // so no bundle-supplied identifier ever reaches the SQL text.
            let sql = format!(
                "INSERT INTO {table} SELECT * FROM jsonb_populate_record(NULL::{table}, $1)"
            );
            sqlx::query(&sql).bind(&row).execute(&mut *tx).await?;
            count += 1;
        }
        imported.push(TableCount {
            table: (*table).to_string(),
            rows: count,
        });
    }

    tx.commit().await?;
    Ok(imported)
}

/// Swap the source-encrypted `key_secret` for the plaintext key under
/// `key_secret_plain`, so the target can re-encrypt it under its own secret.
/// A key stored before at-rest encryption existed has nothing to recover; it
/// still authenticates after import, it just cannot be revealed.
fn rekey_for_export(row: &mut serde_json::Value) {
    let Some(obj) = row.as_object_mut() else {
        return;
    };
    let stored = obj
        .remove("key_secret")
        .and_then(|v| v.as_str().map(str::to_owned));
    if let Some(plain) = stored.and_then(|s| super::api_keys::decrypt_api_key_for_transfer(&s)) {
        obj.insert("key_secret_plain".into(), serde_json::Value::String(plain));
    }
}

// ── Import ──────────────────────────────────────────────────────────────

async fn import_config(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportSummary>, AppError> {
    require_admin(&claims)?;

    if req.bundle.format != FORMAT {
        return Err(AppError::BadRequest(
            "This file is not an MCP Gateway configuration bundle".into(),
        ));
    }
    if req.bundle.format_version > FORMAT_VERSION {
        return Err(AppError::BadRequest(format!(
            "Bundle format v{} is newer than this server supports (v{FORMAT_VERSION}). Upgrade the gateway first.",
            req.bundle.format_version
        )));
    }

    let payload: Payload = serde_json::from_slice(&gunzip(&unseal(&req.bundle, &req.passphrase)?)?)
        .map_err(|_| AppError::BadRequest("Bundle payload is not readable".into()))?;

    let imported = restore_payload(&state.db, &payload).await?;

    let total: i64 = imported.iter().map(|t| t.rows).sum();
    tracing::warn!(
        actor = %claims.sub,
        rows = total,
        source_version = %req.bundle.source_version,
        "Configuration import completed — all prior data was replaced"
    );
    // Recorded after the wipe so the import itself survives in the new audit log.
    record_transfer(&state, &claims, "config.import", total).await;

    Ok(Json(ImportSummary {
        imported,
        source_version: req.bundle.source_version.clone(),
        created_at: req.bundle.created_at.clone(),
    }))
}

/// Re-encrypt the exported plaintext key under *this* deployment's secret.
fn rekey_for_import(row: &mut serde_json::Value) -> Result<(), AppError> {
    let Some(obj) = row.as_object_mut() else {
        return Ok(());
    };
    let plain = obj
        .remove("key_secret_plain")
        .and_then(|v| v.as_str().map(str::to_owned));
    if let Some(plain) = plain {
        let sealed = super::api_keys::encrypt_api_key(&plain)?;
        obj.insert("key_secret".into(), serde_json::Value::String(sealed));
    }
    Ok(())
}

/// Best-effort audit row for the transfer itself. A failure to record must not
/// fail the operation that already succeeded, but it must be visible.
async fn record_transfer(state: &AppState, claims: &Claims, action: &str, rows: i64) {
    let user_id = uuid::Uuid::parse_str(&claims.sub).ok();
    let res = sqlx::query(
        "INSERT INTO audit_events
           (event_id, trace_id, user_id, tool_name, backend_name, status, risk_category, metadata)
         VALUES ($1, $2, $3, $4, 'system', 'success', 'admin', $5)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(uuid::Uuid::now_v7())
    .bind(user_id)
    .bind(action)
    .bind(serde_json::json!({ "rows": rows }))
    .execute(&state.db)
    .await;
    if let Err(e) = res {
        tracing::error!(error = %e, action, "Failed to record config transfer in the audit log");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::{lock_env, lock_env_async};

    fn params() -> KdfParams {
        KdfParams {
            algorithm: "argon2id".into(),
            salt: B64.encode([7u8; 16]),
            // Deliberately cheap so the test suite stays fast.
            m_cost: 8,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn bundle_for(plaintext: &[u8], passphrase: &str) -> Bundle {
        let p = params();
        let salt = B64.decode(&p.salt).unwrap();
        let key = derive_key(passphrase, &salt, &p).unwrap();
        let nonce = [3u8; 12];
        let ct = ChaCha20Poly1305::new(Key::from_slice(&key))
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .unwrap();
        Bundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            created_at: "2026-01-01T00:00:00Z".into(),
            source_version: "test".into(),
            includes_audit: false,
            kdf: p,
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(ct),
        }
    }

    #[test]
    fn seal_then_unseal_round_trips() {
        let secret = b"backend token: hunter2";
        let (kdf, nonce, ciphertext) = seal(secret, "correct horse battery").unwrap();
        let bundle = Bundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            created_at: "now".into(),
            source_version: "test".into(),
            includes_audit: false,
            kdf,
            nonce,
            ciphertext,
        };
        assert_eq!(
            unseal(&bundle, "correct horse battery").unwrap(),
            secret.to_vec()
        );
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let bundle = bundle_for(b"top secret", "the right passphrase");
        let err = unseal(&bundle, "the wrong passphrase").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        // AEAD must catch a flipped byte rather than yielding garbage plaintext.
        let mut bundle = bundle_for(b"top secret", "the right passphrase");
        let mut raw = B64.decode(&bundle.ciphertext).unwrap();
        raw[0] ^= 0xff;
        bundle.ciphertext = B64.encode(raw);
        assert!(unseal(&bundle, "the right passphrase").is_err());
    }

    #[test]
    fn bundle_does_not_leak_plaintext() {
        let secret = b"OBSIDIAN_API_KEY=super-secret-value";
        let (kdf, nonce, ciphertext) = seal(secret, "a good long passphrase").unwrap();
        let json = serde_json::to_string(&Bundle {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            created_at: "now".into(),
            source_version: "test".into(),
            includes_audit: true,
            kdf,
            nonce,
            ciphertext,
        })
        .unwrap();
        assert!(!json.contains("super-secret-value"));
        assert!(!json.contains("OBSIDIAN_API_KEY"));
    }

    #[test]
    fn gzip_round_trips() {
        let data = b"{\"tables\":{}}".repeat(100);
        assert_eq!(gunzip(&gzip(&data).unwrap()).unwrap(), data);
    }

    #[test]
    fn absurd_kdf_parameters_are_rejected() {
        // A hostile bundle must not be able to make us allocate gigabytes.
        let p = KdfParams {
            algorithm: "argon2id".into(),
            salt: B64.encode([0u8; 16]),
            m_cost: 4_000_000,
            t_cost: 1,
            p_cost: 1,
        };
        assert!(derive_key("passphrase", &[0u8; 16], &p).is_err());
    }

    #[test]
    fn unknown_kdf_is_rejected() {
        let p = KdfParams {
            algorithm: "scrypt".into(),
            salt: B64.encode([0u8; 16]),
            m_cost: 8,
            t_cost: 1,
            p_cost: 1,
        };
        assert!(derive_key("passphrase", &[0u8; 16], &p).is_err());
    }

    #[test]
    fn export_rekey_replaces_stored_blob_with_plaintext() {
        let _guard = lock_env();
        // Guards the actual 1:1 requirement: the exported row must not carry the
        // source-encrypted blob, which would be undecryptable on the target.
        std::env::set_var("JWT_SECRET", "test-secret-for-transfer-rekey");
        let sealed = super::super::api_keys::encrypt_api_key("mcpgw_abc123def456").unwrap();
        let mut row = serde_json::json!({ "key_id": "x", "key_secret": sealed });
        rekey_for_export(&mut row);
        assert!(
            row.get("key_secret").is_none(),
            "stored blob must be dropped"
        );
        assert_eq!(row["key_secret_plain"], "mcpgw_abc123def456");
    }

    #[test]
    fn import_rekey_reseals_under_local_secret() {
        let _guard = lock_env();
        std::env::set_var("JWT_SECRET", "test-secret-for-transfer-rekey");
        let mut row = serde_json::json!({ "key_secret_plain": "mcpgw_abc123def456" });
        rekey_for_import(&mut row).unwrap();
        assert!(row.get("key_secret_plain").is_none());
        let sealed = row["key_secret"].as_str().unwrap();
        assert_ne!(sealed, "mcpgw_abc123def456", "must not store plaintext");
        assert_eq!(
            super::super::api_keys::decrypt_api_key_for_transfer(sealed).unwrap(),
            "mcpgw_abc123def456"
        );
    }

    // ── Database round-trip ─────────────────────────────────────────────
    //
    // These exercise the part unit tests cannot reach: that a bundle actually
    // reproduces a deployment. They need a real Postgres because the transfer is
    // built on `row_to_json` / `jsonb_populate_record`, which have no in-process
    // equivalent. CI provides one; locally they skip unless TEST_DATABASE_URL is
    // set, so `cargo test` still works with no database.

    async fn test_pool() -> Option<sqlx::PgPool> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("TEST_DATABASE_URL is set but unreachable");
        crate::db::run_migrations(&pool)
            .await
            .expect("migrations should apply");
        // Start from a known-empty deployment.
        let mut ordered: Vec<&str> = TABLES.to_vec();
        ordered.push(AUDIT_TABLE);
        for table in ordered.iter().rev() {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        Some(pool)
    }

    async fn seed_fixture(pool: &sqlx::PgPool) -> (uuid::Uuid, uuid::Uuid) {
        let role_id = uuid::Uuid::new_v4();
        let user_id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO roles (role_id, name, description, permissions, is_system, default_policy)
             VALUES ($1, 'transfer-test-role', 'fixture', '[]'::jsonb, FALSE, 'deny')",
        )
        .bind(role_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (user_id, username, password_hash, email, is_active)
             VALUES ($1, 'transfer-test-user', '$argon2id$fixture', 'u@example.com', TRUE)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(role_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO api_keys (key_id, user_id, key_hash, key_prefix, name, is_active, key_secret)
             VALUES ($1, $2, 'deadbeef', 'mcpgw_', 'fixture-key', TRUE, $3)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(user_id)
        .bind(super::super::api_keys::encrypt_api_key("mcpgw_fixture_raw_key").unwrap())
        .execute(pool)
        .await
        .unwrap();
        (user_id, role_id)
    }

    #[tokio::test]
    async fn db_round_trip_restores_every_table() {
        let _guard = lock_env_async().await;
        std::env::set_var("JWT_SECRET", "source-secret-for-db-round-trip");
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };
        seed_fixture(&pool).await;

        let before = collect_payload(&pool, true).await.unwrap();
        assert_eq!(before.tables["users"].len(), 1);
        assert_eq!(before.tables["api_keys"].len(), 1);

        // Wipe, then restore from the payload.
        restore_payload(&pool, &before).await.unwrap();
        let after = collect_payload(&pool, true).await.unwrap();

        for table in TABLES {
            assert_eq!(
                before.tables[*table].len(),
                after.tables[*table].len(),
                "row count changed for {table}"
            );
        }
        let (username,): (String,) =
            sqlx::query_as("SELECT username FROM users WHERE username = 'transfer-test-user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(username, "transfer-test-user");
    }

    #[tokio::test]
    async fn db_round_trip_keeps_api_keys_revealable_under_a_new_secret() {
        // The headline promise of the feature. Export under one JWT_SECRET,
        // import under another, and the key must still decrypt on the target.
        let _guard = lock_env_async().await;
        std::env::set_var("JWT_SECRET", "source-secret-for-db-round-trip");
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };
        seed_fixture(&pool).await;

        let payload = collect_payload(&pool, false).await.unwrap();

        std::env::set_var("JWT_SECRET", "target-secret-for-db-round-trip");
        restore_payload(&pool, &payload).await.unwrap();

        let (stored,): (Option<String>,) =
            sqlx::query_as("SELECT key_secret FROM api_keys WHERE name = 'fixture-key'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored = stored.expect("key_secret should have been re-sealed on import");
        assert_eq!(
            super::super::api_keys::decrypt_api_key_for_transfer(&stored).unwrap(),
            "mcpgw_fixture_raw_key",
            "key must be revealable under the target's secret"
        );
    }

    #[tokio::test]
    async fn db_import_replaces_rather_than_merges() {
        let _guard = lock_env_async().await;
        std::env::set_var("JWT_SECRET", "source-secret-for-db-round-trip");
        let Some(pool) = test_pool().await else {
            eprintln!("skipping: TEST_DATABASE_URL not set");
            return;
        };
        seed_fixture(&pool).await;
        let payload = collect_payload(&pool, false).await.unwrap();

        // A user that exists only on the target must not survive the import.
        sqlx::query(
            "INSERT INTO users (user_id, username, password_hash, is_active)
             VALUES ($1, 'target-only-user', 'x', TRUE)",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        restore_payload(&pool, &payload).await.unwrap();

        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM users WHERE username = 'target-only-user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 0, "replace mode must delete rows absent from the bundle");
    }

    #[test]
    fn key_rotation_across_deployments_survives() {
        let _guard = lock_env();
        // The whole point of the feature: export on one JWT_SECRET, import under
        // another, and the key is still revealable on the target.
        std::env::set_var("JWT_SECRET", "source-deployment-secret-value");
        let sealed_at_source = super::super::api_keys::encrypt_api_key("mcpgw_portable").unwrap();
        let mut row = serde_json::json!({ "key_secret": sealed_at_source });
        rekey_for_export(&mut row);

        std::env::set_var("JWT_SECRET", "target-deployment-secret-value");
        rekey_for_import(&mut row).unwrap();
        assert_eq!(
            super::super::api_keys::decrypt_api_key_for_transfer(
                row["key_secret"].as_str().unwrap()
            )
            .unwrap(),
            "mcpgw_portable"
        );
    }
}

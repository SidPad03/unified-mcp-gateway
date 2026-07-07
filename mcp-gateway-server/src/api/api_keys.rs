use axum::{
    extract::{Path, State},
    routing::{get, post, delete},
    Json, Router,
};
use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, Key, KeyInit, Nonce};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AppError, AppState};
use super::auth::{Claims, require_admin};

pub const SUPPORTED_APPS: &[&str] = &["claude", "claudedesktop", "cursor", "vscode", "openwebui", "clawbot", "codex", "lmstudio"];

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub user_id: Option<String>,
    pub application: Option<String>,
}

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    pub key_id: String,
    pub raw_key: String,
    pub key_prefix: String,
    pub name: String,
    pub user_id: String,
    pub application: Option<String>,
}

#[derive(Serialize)]
pub struct ApiKeyResponse {
    pub key_id: String,
    pub key_prefix: String,
    pub name: String,
    pub user_id: String,
    pub username: String,
    pub is_active: bool,
    pub created_at: String,
    pub last_used: Option<String>,
    pub expires_at: Option<String>,
    pub application: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api-keys", post(create_api_key).get(list_api_keys))
        .route("/api-keys/:id", delete(delete_api_key).patch(update_api_key))
        .route("/api-keys/provision/:user_id", post(provision_app_keys))
        .route("/api-keys/by-user/:user_id", get(keys_by_user))
        .route("/api-keys/reveal/:user_id", post(reveal_app_keys))
}

async fn create_api_key(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, AppError> {
    require_admin(&claims)?;

    let target_user_id: Uuid = req
        .user_id
        .as_deref()
        .unwrap_or(&claims.sub)
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    // Verify user exists
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM users WHERE user_id = $1",
    )
    .bind(target_user_id)
    .fetch_optional(&state.db)
    .await?;

    if exists.is_none() {
        return Err(AppError::NotFound("User not found".into()));
    }

    // Generate key: mcpgw_ + 48 random alphanumeric chars
    let raw_key = generate_api_key();
    let key_prefix = raw_key[..12].to_string();
    let key_hash = hash_key(&raw_key);
    let key_secret = encrypt_api_key(&raw_key).ok();
    let key_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO api_keys (key_id, user_id, key_hash, key_prefix, name, application, key_secret) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(key_id)
    .bind(target_user_id)
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(&req.name)
    .bind(&req.application)
    .bind(&key_secret)
    .execute(&state.db)
    .await?;

    Ok(Json(CreateApiKeyResponse {
        key_id: key_id.to_string(),
        raw_key,
        key_prefix,
        name: req.name,
        user_id: target_user_id.to_string(),
        application: req.application,
    }))
}

async fn list_api_keys(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<Vec<ApiKeyResponse>>, AppError> {
    let is_admin = claims.roles.contains(&"owner".to_string());

    let keys: Vec<(Uuid, String, String, Uuid, String, bool, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> = if is_admin {
        sqlx::query_as(
            "SELECT ak.key_id, ak.key_prefix, ak.name, ak.user_id, u.username, ak.is_active, ak.created_at, ak.last_used, ak.expires_at, ak.application
             FROM api_keys ak JOIN users u ON ak.user_id = u.user_id
             ORDER BY ak.created_at DESC"
        )
        .fetch_all(&state.db)
        .await?
    } else {
        let user_id: Uuid = claims.sub.parse().map_err(|_| AppError::Internal("Invalid user_id in claims".into()))?;
        sqlx::query_as(
            "SELECT ak.key_id, ak.key_prefix, ak.name, ak.user_id, u.username, ak.is_active, ak.created_at, ak.last_used, ak.expires_at, ak.application
             FROM api_keys ak JOIN users u ON ak.user_id = u.user_id
             WHERE ak.user_id = $1
             ORDER BY ak.created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await?
    };

    let result: Vec<ApiKeyResponse> = keys
        .into_iter()
        .map(|(key_id, prefix, name, user_id, username, active, created, last_used, expires, application)| {
            ApiKeyResponse {
                key_id: key_id.to_string(),
                key_prefix: prefix,
                name,
                user_id: user_id.to_string(),
                username,
                is_active: active,
                created_at: created.to_rfc3339(),
                last_used: last_used.map(|t| t.to_rfc3339()),
                expires_at: expires.map(|t| t.to_rfc3339()),
                application,
            }
        })
        .collect();

    Ok(Json(result))
}

async fn delete_api_key(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let key_id: Uuid = id.parse().map_err(|_| AppError::BadRequest("Invalid key_id".into()))?;

    let result = sqlx::query("DELETE FROM api_keys WHERE key_id = $1")
        .bind(key_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("API key not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct UpdateApiKeyRequest {
    pub name: String,
}

async fn update_api_key(
    State(state): State<AppState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(req): Json<UpdateApiKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&claims)?;

    let key_id: Uuid = id.parse().map_err(|_| AppError::BadRequest("Invalid key_id".into()))?;

    let result = sqlx::query("UPDATE api_keys SET name = $1 WHERE key_id = $2")
        .bind(&req.name)
        .bind(key_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("API key not found".into()));
    }

    Ok(Json(serde_json::json!({ "status": "updated" })))
}

pub(crate) fn generate_api_key() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random: String = (0..48)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("mcpgw_{}", random)
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// ChaCha20-Poly1305 cipher for encrypting API keys at rest, keyed by a SHA-256
/// of `JWT_SECRET` (guaranteed present and >= 16 chars by startup validation).
/// The hash of the key stays the auth credential; the encrypted copy only exists
/// so the dashboard can reveal a key for the "copy client config" flow. A
/// DB-only leak can't recover keys without the secret.
fn key_cipher() -> Result<ChaCha20Poly1305, AppError> {
    let secret = std::env::var("JWT_SECRET")
        .map_err(|_| AppError::Internal("JWT_SECRET not configured".into()))?;
    let digest = Sha256::digest(secret.as_bytes());
    Ok(ChaCha20Poly1305::new(Key::from_slice(digest.as_slice())))
}

/// Encrypt a raw API key for at-rest storage. Returns `hex(nonce || ciphertext)`.
pub(crate) fn encrypt_api_key(raw_key: &str) -> Result<String, AppError> {
    let cipher = key_cipher()?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), raw_key.as_bytes())
        .map_err(|_| AppError::Internal("Failed to encrypt API key".into()))?;
    let mut blob = nonce_bytes.to_vec();
    blob.extend_from_slice(&ciphertext);
    Ok(hex::encode(blob))
}

/// Decrypt a value produced by [`encrypt_api_key`].
fn decrypt_api_key(stored: &str) -> Result<String, AppError> {
    let blob = hex::decode(stored).map_err(|_| AppError::Internal("Corrupt stored key".into()))?;
    if blob.len() <= 12 {
        return Err(AppError::Internal("Corrupt stored key".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let plaintext = key_cipher()?
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| AppError::Internal("Failed to decrypt API key".into()))?;
    String::from_utf8(plaintext).map_err(|_| AppError::Internal("Corrupt stored key".into()))
}

async fn provision_app_keys(
    State(state): State<AppState>,
    claims: Claims,
    Path(user_id_str): Path<String>,
) -> Result<Json<Vec<CreateApiKeyResponse>>, AppError> {
    let target_user_id: Uuid = user_id_str
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    // Only admin or self
    let is_self = claims.sub == target_user_id.to_string();
    if !is_self {
        require_admin(&claims)?;
    }

    // Verify user exists
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT user_id FROM users WHERE user_id = $1")
        .bind(target_user_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("User not found".into()));
    }

    let mut result = Vec::new();
    for app in SUPPORTED_APPS {
        let raw_key = generate_api_key();
        let key_prefix = raw_key[..12].to_string();
        let key_hash = hash_key(&raw_key);
        let key_secret = encrypt_api_key(&raw_key).ok();
        let key_id = Uuid::new_v4();
        let key_name = format!("{}-key", app);

        let insert_result = sqlx::query(
            "INSERT INTO api_keys (key_id, user_id, key_hash, key_prefix, name, application, key_secret)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, application) WHERE application IS NOT NULL DO NOTHING"
        )
        .bind(key_id)
        .bind(target_user_id)
        .bind(&key_hash)
        .bind(&key_prefix)
        .bind(&key_name)
        .bind(*app)
        .bind(&key_secret)
        .execute(&state.db)
        .await?;

        if insert_result.rows_affected() > 0 {
            result.push(CreateApiKeyResponse {
                key_id: key_id.to_string(),
                raw_key,
                key_prefix,
                name: key_name,
                user_id: target_user_id.to_string(),
                application: Some(app.to_string()),
            });
        }
    }

    Ok(Json(result))
}

async fn keys_by_user(
    State(state): State<AppState>,
    claims: Claims,
    Path(user_id_str): Path<String>,
) -> Result<Json<Vec<ApiKeyResponse>>, AppError> {
    let target_user_id: Uuid = user_id_str
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    let is_self = claims.sub == target_user_id.to_string();
    if !is_self {
        require_admin(&claims)?;
    }

    let keys: Vec<(Uuid, String, String, Uuid, String, bool, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> =
        sqlx::query_as(
            "SELECT ak.key_id, ak.key_prefix, ak.name, ak.user_id, u.username, ak.is_active, ak.created_at, ak.last_used, ak.expires_at, ak.application
             FROM api_keys ak JOIN users u ON ak.user_id = u.user_id
             WHERE ak.user_id = $1
             ORDER BY ak.application NULLS LAST, ak.created_at DESC"
        )
        .bind(target_user_id)
        .fetch_all(&state.db)
        .await?;

    let result: Vec<ApiKeyResponse> = keys
        .into_iter()
        .map(|(key_id, prefix, name, user_id, username, active, created, last_used, expires, application)| {
            ApiKeyResponse {
                key_id: key_id.to_string(),
                key_prefix: prefix,
                name,
                user_id: user_id.to_string(),
                username,
                is_active: active,
                created_at: created.to_rfc3339(),
                last_used: last_used.map(|t| t.to_rfc3339()),
                expires_at: expires.map(|t| t.to_rfc3339()),
                application,
            }
        })
        .collect();

    Ok(Json(result))
}

#[derive(Serialize)]
pub struct RevealedKey {
    pub key_id: String,
    pub application: Option<String>,
    pub key_prefix: String,
    pub raw_key: String,
}

/// Reveal a user's per-application API keys in full so the dashboard can build a
/// ready-to-paste client config. Allowed for the key owner (self) or an admin.
/// Keys stored before at-rest encryption existed have no recoverable value and
/// are rotated once here so they become revealable — safe, because such keys
/// were never shown to the user in the first place.
async fn reveal_app_keys(
    State(state): State<AppState>,
    claims: Claims,
    Path(user_id_str): Path<String>,
) -> Result<Json<Vec<RevealedKey>>, AppError> {
    let target_user_id: Uuid = user_id_str
        .parse()
        .map_err(|_| AppError::BadRequest("Invalid user_id".into()))?;

    let is_self = claims.sub == target_user_id.to_string();
    if !is_self {
        require_admin(&claims)?;
    }

    let rows: Vec<(Uuid, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT key_id, application, key_prefix, key_secret FROM api_keys
         WHERE user_id = $1 AND is_active = TRUE AND application IS NOT NULL
         ORDER BY application"
    )
    .bind(target_user_id)
    .fetch_all(&state.db)
    .await?;

    let mut result = Vec::with_capacity(rows.len());
    for (key_id, application, key_prefix, key_secret) in rows {
        match key_secret.as_deref().and_then(|s| decrypt_api_key(s).ok()) {
            Some(raw_key) => result.push(RevealedKey {
                key_id: key_id.to_string(),
                application,
                key_prefix,
                raw_key,
            }),
            None => {
                // Legacy hash-only key: rotate in place to make it revealable.
                let new_raw = generate_api_key();
                let new_prefix = new_raw[..12].to_string();
                let new_hash = hash_key(&new_raw);
                let new_secret = encrypt_api_key(&new_raw).ok();
                sqlx::query(
                    "UPDATE api_keys SET key_hash = $1, key_prefix = $2, key_secret = $3 WHERE key_id = $4"
                )
                .bind(&new_hash)
                .bind(&new_prefix)
                .bind(&new_secret)
                .bind(key_id)
                .execute(&state.db)
                .await?;
                result.push(RevealedKey {
                    key_id: key_id.to_string(),
                    application,
                    key_prefix: new_prefix,
                    raw_key: new_raw,
                });
            }
        }
    }

    Ok(Json(result))
}

pub async fn generate_app_keys_for_user(pool: &sqlx::PgPool, user_id: Uuid) -> Result<(), sqlx::Error> {
    for app in SUPPORTED_APPS {
        let raw_key = generate_api_key();
        let key_prefix = raw_key[..12].to_string();
        let key_hash = hash_key(&raw_key);
        let key_secret = encrypt_api_key(&raw_key).ok();
        let key_id = Uuid::new_v4();
        let key_name = format!("{}-key", app);

        sqlx::query(
            "INSERT INTO api_keys (key_id, user_id, key_hash, key_prefix, name, application, key_secret)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id, application) WHERE application IS NOT NULL DO NOTHING"
        )
        .bind(key_id)
        .bind(user_id)
        .bind(&key_hash)
        .bind(&key_prefix)
        .bind(&key_name)
        .bind(*app)
        .bind(&key_secret)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_api_key_roundtrip() {
        std::env::set_var("JWT_SECRET", "test-secret-at-least-16-chars-long");
        let raw = generate_api_key();

        let stored = encrypt_api_key(&raw).expect("encrypt should succeed");
        // Stored form is ciphertext, never the plaintext key.
        assert_ne!(stored, raw);
        assert!(!stored.contains(&raw));

        let recovered = decrypt_api_key(&stored).expect("decrypt should succeed");
        assert_eq!(recovered, raw);

        // Random nonce => encrypting the same key twice yields different blobs,
        // both of which still decrypt back to the original.
        let stored2 = encrypt_api_key(&raw).expect("encrypt should succeed");
        assert_ne!(stored, stored2);
        assert_eq!(decrypt_api_key(&stored2).expect("decrypt"), raw);

        // Tampered ciphertext must fail the Poly1305 authentication tag.
        let mut tampered = stored.clone();
        tampered.pop();
        tampered.push(if stored.ends_with('a') { 'b' } else { 'a' });
        assert!(decrypt_api_key(&tampered).is_err());
    }
}

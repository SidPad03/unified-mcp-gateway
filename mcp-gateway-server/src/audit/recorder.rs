use sqlx::PgPool;
use uuid::Uuid;
use chrono::Utc;
use sha2::{Sha256, Digest};
use tokio::sync::broadcast;

use super::redactor::Redactor;

pub struct AuditRecorder {
    pool: PgPool,
    redactor: Redactor,
    event_tx: broadcast::Sender<String>,
}

impl AuditRecorder {
    pub fn new(pool: PgPool, event_tx: broadcast::Sender<String>) -> Self {
        Self {
            pool,
            redactor: Redactor::new(),
            event_tx,
        }
    }

    pub async fn record_event(
        &self,
        tool_name: &str,
        backend_name: &str,
        risk_category: &str,
        request_payload: Option<&str>,
        response_payload: Option<&str>,
        duration_ms: f64,
        status: &str,
        error_message: Option<&str>,
        policy_decision: &str,
        policy_id: Option<&str>,
        user_id: Option<Uuid>,
        session_id: Option<&str>,
        client_id: Option<&str>,
        application: Option<&str>,
    ) -> Result<Uuid, sqlx::Error> {
        let event_id = Uuid::now_v7();
        let trace_id = Uuid::now_v7();

        let request_hash = request_payload.map(|p| hash_payload(p));
        let response_hash = response_payload.map(|p| hash_payload(p));

        // Redact payloads before storage
        let redacted_request = request_payload.map(|p| self.redactor.redact(p));
        let redacted_response = response_payload.map(|p| self.redactor.redact(p));

        let policy_uuid: Option<Uuid> = policy_id.and_then(|id| id.parse().ok());

        sqlx::query(
            "INSERT INTO audit_events (event_id, timestamp, trace_id, session_id, user_id, client_id, tool_name, backend_name, risk_category, request_hash, request_payload, response_hash, response_payload, duration_ms, status, error_message, policy_decision, policy_id, risk_flags, metadata, application)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, '[]'::jsonb, '{}'::jsonb, $19)"
        )
        .bind(event_id)
        .bind(Utc::now())
        .bind(trace_id)
        .bind(session_id)
        .bind(user_id)
        .bind(client_id)
        .bind(tool_name)
        .bind(backend_name)
        .bind(risk_category)
        .bind(request_hash)
        .bind(redacted_request)
        .bind(response_hash)
        .bind(redacted_response)
        .bind(duration_ms)
        .bind(status)
        .bind(error_message)
        .bind(policy_decision)
        .bind(policy_uuid)
        .bind(application)
        .execute(&self.pool)
        .await?;

        // Broadcast live event to connected dashboard clients (redact sensitive fields)
        let redacted_error = error_message.map(|e| self.redactor.redact(e));
        let live = serde_json::json!({
            "type": "tool_call",
            "tool_name": tool_name,
            "backend_name": backend_name,
            "application": application,
            "risk_category": risk_category,
            "timestamp": Utc::now().to_rfc3339(),
            "status": status,
            "duration_ms": duration_ms,
            "error_message": redacted_error,
            "user_id": user_id.map(|u| u.to_string()),
        });
        let _ = self.event_tx.send(live.to_string());

        Ok(event_id)
    }
}

fn hash_payload(payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

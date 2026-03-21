use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub user_id: Option<Uuid>,
    pub client_id: String,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(&self, client_id: &str, user_id: Option<Uuid>) -> Session {
        let session = Session {
            session_id: Uuid::new_v4().to_string(),
            user_id,
            client_id: client_id.to_string(),
            created_at: Utc::now(),
            last_active: Utc::now(),
        };
        self.sessions.write().await.insert(session.session_id.clone(), session.clone());
        session
    }

    pub async fn get_session(&self, session_id: &str) -> Option<Session> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

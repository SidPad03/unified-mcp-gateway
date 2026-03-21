use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};

struct BucketEntry {
    tokens: u32,
    last_refill: DateTime<Utc>,
}

pub struct RateLimiter {
    buckets: Arc<RwLock<HashMap<String, BucketEntry>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn check_rate_limit(
        &self,
        key: &str,
        max_calls: u32,
        window_seconds: u32,
    ) -> bool {
        let mut buckets = self.buckets.write().await;
        let now = Utc::now();

        let entry = buckets.entry(key.to_string()).or_insert(BucketEntry {
            tokens: max_calls,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.signed_duration_since(entry.last_refill);
        if elapsed >= Duration::seconds(window_seconds as i64) {
            entry.tokens = max_calls;
            entry.last_refill = now;
        }

        if entry.tokens > 0 {
            entry.tokens -= 1;
            true
        } else {
            false
        }
    }
}

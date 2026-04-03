use std::sync::Arc;

use chrono::Utc;
use tracing::instrument;
use uuid::Uuid;

use shared::{cache::CacheClient, errors::AppResult};

use crate::domain::models::PresenceStatus;

/// Redis key format for a user's presence record.
fn presence_key(user_id: Uuid) -> String {
    format!("presence:{}", user_id)
}

/// Pub/sub channel for broadcasting presence changes.
const PRESENCE_CHANNEL: &str = "presence_changes";

/// Serializable presence event (mirrors chat_service::PresenceEvent).
#[derive(serde::Serialize, serde::Deserialize)]
struct PresenceEvent {
    user_id: Uuid,
    online: bool,
    last_seen: chrono::DateTime<Utc>,
}

pub struct PresenceService {
    cache: Arc<CacheClient>,
    presence_ttl_secs: u64,
}

impl PresenceService {
    pub fn new(cache: Arc<CacheClient>, presence_ttl_secs: u64) -> Self {
        Self {
            cache,
            presence_ttl_secs,
        }
    }

    /// Mark a user as online. Called by the chat service on WebSocket connect,
    /// or by clients via the heartbeat endpoint.
    #[instrument(skip(self))]
    pub async fn set_online(&self, user_id: Uuid) -> AppResult<()> {
        let status = PresenceStatus {
            user_id,
            online: true,
            last_seen: Utc::now(),
        };
        // TTL = presence_ttl_secs (default 60s). Client must heartbeat to stay online.
        self.cache
            .set(&presence_key(user_id), &status, self.presence_ttl_secs)
            .await?;
        self.publish_event(user_id, true).await
    }

    /// Mark a user as offline. Called on WebSocket disconnect.
    #[instrument(skip(self))]
    pub async fn set_offline(&self, user_id: Uuid) -> AppResult<()> {
        let status = PresenceStatus {
            user_id,
            online: false,
            last_seen: Utc::now(),
        };
        // Keep the offline record for 1 hour so "last seen" queries are accurate.
        self.cache
            .set(&presence_key(user_id), &status, 3600)
            .await?;
        self.publish_event(user_id, false).await
    }

    /// Heartbeat — refresh TTL for an already-online user.
    /// Called every 30s from the client to prevent expiry.
    pub async fn heartbeat(&self, user_id: Uuid) -> AppResult<()> {
        // Re-write to reset TTL. If key is gone (expired), create fresh online record.
        let status = PresenceStatus {
            user_id,
            online: true,
            last_seen: Utc::now(),
        };
        self.cache
            .set(&presence_key(user_id), &status, self.presence_ttl_secs)
            .await
    }

    /// Get presence for a single user. Returns None if user has never been seen.
    pub async fn get_presence(&self, user_id: Uuid) -> AppResult<Option<PresenceStatus>> {
        self.cache
            .get::<PresenceStatus>(&presence_key(user_id))
            .await
    }

    /// Batch query — returns one entry per user found. Missing users are omitted.
    /// Used by clients on connection to bootstrap the online indicator for all
    /// visible users in their conversation list.
    pub async fn get_batch(&self, user_ids: Vec<Uuid>) -> AppResult<Vec<PresenceStatus>> {
        let mut results = Vec::with_capacity(user_ids.len());
        // TODO: pipeline Redis mget for efficiency when batch sizes grow.
        for user_id in user_ids {
            if let Some(status) = self.cache.get(&presence_key(user_id)).await? {
                results.push(status);
            }
        }
        Ok(results)
    }

    async fn publish_event(&self, user_id: Uuid, online: bool) -> AppResult<()> {
        let event = PresenceEvent {
            user_id,
            online,
            last_seen: Utc::now(),
        };
        let payload = serde_json::to_string(&event)
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;
        self.cache.publish(PRESENCE_CHANNEL, &payload).await
    }
}

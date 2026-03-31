use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceStatus {
    pub user_id: Uuid,
    pub online: bool,
    /// Timestamp of last heartbeat/disconnect.
    pub last_seen: DateTime<Utc>,
}

/// POST /api/v1/presence/batch — query multiple users at once.
#[derive(Debug, Deserialize)]
pub struct BatchPresenceRequest {
    pub user_ids: Vec<Uuid>,
}

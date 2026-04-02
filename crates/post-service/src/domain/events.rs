use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Published to Kafka when a post is created.
/// Consumed by feed-service to trigger fan-out to followers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostCreated {
    pub post_id: Uuid,
    pub author_id: Uuid,
    /// Unix epoch milliseconds — used as sorted set score in Redis
    pub timestamp_ms: i64,
}

/// Published when a post is deleted.
/// Consumed by feed-service to remove from affected feeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostDeleted {
    pub post_id: Uuid,
    pub author_id: Uuid,
}

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Consumed from Kafka topic "post.events"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostCreated {
    pub post_id: Uuid,
    pub author_id: Uuid,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostDeleted {
    pub post_id: Uuid,
    pub author_id: Uuid,
}

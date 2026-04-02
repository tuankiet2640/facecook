use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Published to Kafka when a message is sent but recipient is offline.
/// Consumed by notification service to push a mobile push notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageQueued {
    pub message_id: Uuid,
    pub conversation_id: Uuid,
    pub sender_id: Uuid,
    pub recipient_id: Uuid,
    pub content_preview: String,
}

/// Published when a message is delivered (for receipts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelivered {
    pub message_id: Uuid,
    pub conversation_id: Uuid,
    pub delivered_to: Uuid,
}

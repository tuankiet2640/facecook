use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use shared::models::message::MessageType;

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    pub conversation_id: Uuid,
    #[validate(length(min = 1, max = 100_000))]
    pub content: String,
    pub message_type: Option<MessageType>,
    /// Client-generated UUID for deduplication
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub participant_id: Uuid,
}

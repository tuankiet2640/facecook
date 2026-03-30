use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub sender_id: Uuid,
    pub content: String,
    pub message_type: MessageType,
    /// Monotonically increasing per-conversation counter for ordering.
    /// Generated via Redis INCR to avoid DB round-trips on hot path.
    pub sequence_number: i64,
    /// Client-provided UUID for exactly-once delivery guarantees.
    pub idempotency_key: String,
    pub delivered_at: Option<DateTime<Utc>>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "message_type", rename_all = "lowercase")]
pub enum MessageType {
    Text,
    Image,
    Video,
    File,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub participant_a: Uuid,
    pub participant_b: Uuid,
    pub last_message_id: Option<Uuid>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// All WebSocket traffic is typed with this enum.
/// The `type` field discriminates message kind, enabling client-side routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Client → Server: submit a new chat message
    SendMessage {
        /// Client-generated UUID used for idempotency (deduplication)
        id: String,
        conversation_id: Uuid,
        content: String,
        message_type: MessageType,
    },
    /// Server → Client: deliver a message to the recipient
    NewMessage { message: Message },
    /// Client → Server: acknowledge receipt of a message
    Ack { message_id: Uuid },
    /// Server → Client: confirm persistence and delivery
    Delivered {
        message_id: Uuid,
        sequence_number: i64,
    },
    /// Client → Server: keep-alive (every 30s)
    Ping,
    /// Server → Client: response to ping
    Pong,
    /// Server → Client: a followed user's online status changed
    PresenceUpdate {
        user_id: Uuid,
        online: bool,
        last_seen: Option<DateTime<Utc>>,
    },
    /// Server → Client: error notification
    Error { code: String, message: String },
}

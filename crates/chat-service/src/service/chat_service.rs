use std::sync::Arc;

use chrono::Utc;
use metrics::counter;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use shared::{
    cache::CacheClient,
    errors::AppResult,
    kafka::{KafkaEvent, KafkaProducer},
    models::message::{Conversation, Message, MessageType, WsMessage},
};

use crate::{
    domain::events::MessageQueued, repository::message_repo::MessageRepository, ConnectionRegistry,
};

/// Redis key for presence state of a single user.
fn presence_key(user_id: Uuid) -> String {
    format!("presence:{}", user_id)
}

/// Redis pub/sub channel for presence change events.
pub const PRESENCE_CHANNEL: &str = "presence_changes";

/// Serializable presence payload published to Redis on connect/disconnect.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PresenceEvent {
    pub user_id: Uuid,
    pub online: bool,
    pub last_seen: chrono::DateTime<Utc>,
}

pub struct ChatService {
    repo: Arc<MessageRepository>,
    cache: Arc<CacheClient>,
    connections: ConnectionRegistry,
    kafka: Arc<KafkaProducer>,
    chat_topic: String,
}

impl ChatService {
    pub fn new(
        repo: Arc<MessageRepository>,
        cache: Arc<CacheClient>,
        connections: ConnectionRegistry,
        kafka: Arc<KafkaProducer>,
        chat_topic: String,
    ) -> Self {
        Self {
            repo,
            cache,
            connections,
            kafka,
            chat_topic,
        }
    }

    // ── Message lifecycle ────────────────────────────────────────────────────

    /// Send a message from `sender_id` to the other participant of `conversation_id`.
    ///
    /// Delivery guarantee: at-least-once.
    /// - DB persistence is idempotent via `(conversation_id, idempotency_key)` unique constraint.
    /// - Kafka publish is attempted after persistence; partial failures are safe to retry.
    ///
    /// Returns `(persisted_message, sequence_number)`.
    #[instrument(skip(self, content), fields(sender = %sender_id, conv = %conversation_id))]
    pub async fn send_message(
        &self,
        sender_id: Uuid,
        conversation_id: Uuid,
        content: String,
        message_type: MessageType,
        idempotency_key: String,
    ) -> AppResult<(Message, i64)> {
        // 1. Idempotency guard: SET NX with 24h TTL.
        //    A duplicate request (same idempotency_key) returns the cached result
        //    from the DB ON CONFLICT path, so we still return the original message.
        let idem_key = format!("msg:idem:{}:{}", conversation_id, idempotency_key);
        let is_first_attempt = self.cache.set_nx(&idem_key, "1", 86_400).await?;

        // 2. Sequence number via Redis INCR — monotonically increasing per conversation,
        //    no DB round-trip on the hot path. Sequence survives Redis restart if using
        //    persistence; if lost, it resets to 0 — ordering is preserved within a session.
        let seq_key = format!("seq:{}", conversation_id);
        let sequence_number = self.cache.incr(&seq_key).await?;

        // 3. Persist. ON CONFLICT on idempotency_key returns the existing message,
        //    so duplicate requests get idempotent responses.
        let message = self
            .repo
            .insert_message(
                Uuid::new_v4(),
                conversation_id,
                sender_id,
                &content,
                &message_type,
                sequence_number,
                &idempotency_key,
            )
            .await?;

        info!(
            message_id = %message.id,
            seq = sequence_number,
            is_first = is_first_attempt,
            "Message persisted"
        );

        // 4. Attempt direct delivery to online recipient.
        if let Some(conv) = self.repo.get_conversation(conversation_id).await? {
            let recipient_id = if conv.participant_a == sender_id {
                conv.participant_b
            } else {
                conv.participant_a
            };

            if let Some(recipient_tx) = self.connections.get(&recipient_id) {
                recipient_tx
                    .send(WsMessage::NewMessage {
                        message: message.clone(),
                    })
                    .ok();
                counter!("messages_delivered_total", "method" => "websocket").increment(1);
                info!(recipient_id = %recipient_id, "Delivered via WebSocket");
            } else {
                // Recipient offline — queue for push notification / redelivery.
                // Only publish for first attempt to avoid notification storms on retry.
                if is_first_attempt {
                    let event = KafkaEvent::new(
                        "message.queued",
                        MessageQueued {
                            message_id: message.id,
                            conversation_id,
                            sender_id,
                            recipient_id,
                            // Trim preview so Kafka messages stay small
                            content_preview: content.chars().take(100).collect(),
                        },
                    );
                    // Best-effort: failure here doesn't affect persistence.
                    if let Err(e) = self
                        .kafka
                        .publish(&self.chat_topic, &recipient_id.to_string(), &event)
                        .await
                    {
                        warn!(error = %e, "Failed to publish message.queued event — push notification may be delayed");
                    } else {
                        counter!("messages_delivered_total", "method" => "kafka_queued")
                            .increment(1);
                    }
                }
            }
        }

        Ok((message, sequence_number))
    }

    pub async fn mark_delivered(&self, message_id: Uuid) -> AppResult<()> {
        self.repo.mark_delivered(message_id).await
    }

    pub async fn mark_read(&self, conversation_id: Uuid, user_id: Uuid) -> AppResult<()> {
        self.repo.mark_read(conversation_id, user_id).await
    }

    // ── Conversation CRUD ────────────────────────────────────────────────────

    pub async fn get_or_create_conversation(
        &self,
        user_a: Uuid,
        user_b: Uuid,
    ) -> AppResult<Conversation> {
        self.repo.get_or_create_conversation(user_a, user_b).await
    }

    pub async fn get_conversation(&self, conversation_id: Uuid) -> AppResult<Option<Conversation>> {
        self.repo.get_conversation(conversation_id).await
    }

    pub async fn get_conversations(
        &self,
        user_id: Uuid,
        limit: i64,
    ) -> AppResult<Vec<Conversation>> {
        self.repo.get_user_conversations(user_id, limit).await
    }

    pub async fn get_messages(
        &self,
        conversation_id: Uuid,
        before_sequence: Option<i64>,
        limit: i64,
    ) -> AppResult<Vec<Message>> {
        self.repo
            .get_messages(conversation_id, before_sequence, limit)
            .await
    }

    // ── Presence ─────────────────────────────────────────────────────────────

    /// Mark user online in Redis and broadcast via pub/sub.
    pub async fn set_presence_online(&self, user_id: Uuid) -> AppResult<()> {
        let event = PresenceEvent {
            user_id,
            online: true,
            last_seen: Utc::now(),
        };
        // TTL = 60s — heartbeat must refresh before expiry for continuous online status.
        self.cache.set(&presence_key(user_id), &event, 60).await?;
        let payload = serde_json::to_string(&event)
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;
        self.cache.publish(PRESENCE_CHANNEL, &payload).await?;
        Ok(())
    }

    /// Mark user offline and broadcast.
    pub async fn set_presence_offline(&self, user_id: Uuid) -> AppResult<()> {
        let event = PresenceEvent {
            user_id,
            online: false,
            last_seen: Utc::now(),
        };
        self.cache.set(&presence_key(user_id), &event, 3600).await?;
        let payload = serde_json::to_string(&event)
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;
        self.cache.publish(PRESENCE_CHANNEL, &payload).await?;
        Ok(())
    }
}

use uuid::Uuid;

use shared::{
    db::DbPool,
    errors::AppResult,
    models::message::{Conversation, Message, MessageType},
};

pub struct MessageRepository {
    pool: DbPool,
}

impl MessageRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Get or create a conversation between two participants.
    /// Conversation identity is deterministic: same pair always maps to same conversation.
    pub async fn get_or_create_conversation(
        &self,
        participant_a: Uuid,
        participant_b: Uuid,
    ) -> AppResult<Conversation> {
        // Normalize order so LEAST/GREATEST constraint is satisfied
        let (p_a, p_b) = if participant_a.to_string() < participant_b.to_string() {
            (participant_a, participant_b)
        } else {
            (participant_b, participant_a)
        };

        let conversation = sqlx::query_as!(
            Conversation,
            r#"
            INSERT INTO conversations (participant_a, participant_b)
            VALUES ($1, $2)
            ON CONFLICT ON CONSTRAINT conversations_participants_unique
            DO UPDATE SET id = conversations.id
            RETURNING id, participant_a, participant_b, last_message_id, last_message_at, created_at
            "#,
            p_a,
            p_b,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(conversation)
    }

    pub async fn get_conversation(&self, conversation_id: Uuid) -> AppResult<Option<Conversation>> {
        let conv = sqlx::query_as!(
            Conversation,
            r#"
            SELECT id, participant_a, participant_b, last_message_id, last_message_at, created_at
            FROM conversations
            WHERE id = $1
            "#,
            conversation_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(conv)
    }

    pub async fn get_user_conversations(
        &self,
        user_id: Uuid,
        limit: i64,
    ) -> AppResult<Vec<Conversation>> {
        let convs = sqlx::query_as!(
            Conversation,
            r#"
            SELECT id, participant_a, participant_b, last_message_id, last_message_at, created_at
            FROM conversations
            WHERE participant_a = $1 OR participant_b = $1
            ORDER BY last_message_at DESC NULLS LAST
            LIMIT $2
            "#,
            user_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(convs)
    }

    /// Insert a message using an atomically generated sequence number.
    /// Sequence number comes from a Redis INCR (done in ChatService) and is
    /// passed in here. This avoids a DB round-trip for sequence generation.
    pub async fn insert_message(
        &self,
        id: Uuid,
        conversation_id: Uuid,
        sender_id: Uuid,
        content: &str,
        message_type: &MessageType,
        sequence_number: i64,
        idempotency_key: &str,
    ) -> AppResult<Message> {
        let message = sqlx::query_as!(
            Message,
            r#"
            INSERT INTO messages (
                id, conversation_id, sender_id, content, message_type,
                sequence_number, idempotency_key
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (conversation_id, idempotency_key) DO NOTHING
            RETURNING
                id, conversation_id, sender_id, content,
                message_type AS "message_type: MessageType",
                sequence_number, idempotency_key,
                delivered_at, read_at, created_at
            "#,
            id,
            conversation_id,
            sender_id,
            content,
            message_type as &MessageType,
            sequence_number,
            idempotency_key,
        )
        .fetch_optional(&self.pool)
        .await?;

        // If ON CONFLICT triggered, fetch the existing message
        let message = match message {
            Some(m) => m,
            None => {
                sqlx::query_as!(
                    Message,
                    r#"
                    SELECT id, conversation_id, sender_id, content,
                           message_type AS "message_type: MessageType",
                           sequence_number, idempotency_key,
                           delivered_at, read_at, created_at
                    FROM messages
                    WHERE conversation_id = $1 AND idempotency_key = $2
                    "#,
                    conversation_id,
                    idempotency_key
                )
                .fetch_one(&self.pool)
                .await?
            }
        };

        // Update conversation last_message metadata
        sqlx::query!(
            r#"
            UPDATE conversations
            SET last_message_id = $2, last_message_at = NOW()
            WHERE id = $1
            "#,
            conversation_id,
            message.id,
        )
        .execute(&self.pool)
        .await?;

        Ok(message)
    }

    pub async fn get_messages(
        &self,
        conversation_id: Uuid,
        before_sequence: Option<i64>,
        limit: i64,
    ) -> AppResult<Vec<Message>> {
        let messages = if let Some(before_seq) = before_sequence {
            sqlx::query_as!(
                Message,
                r#"
                SELECT id, conversation_id, sender_id, content,
                       message_type AS "message_type: MessageType",
                       sequence_number, idempotency_key,
                       delivered_at, read_at, created_at
                FROM messages
                WHERE conversation_id = $1 AND sequence_number < $2
                ORDER BY sequence_number DESC
                LIMIT $3
                "#,
                conversation_id,
                before_sequence,
                limit
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as!(
                Message,
                r#"
                SELECT id, conversation_id, sender_id, content,
                       message_type AS "message_type: MessageType",
                       sequence_number, idempotency_key,
                       delivered_at, read_at, created_at
                FROM messages
                WHERE conversation_id = $1
                ORDER BY sequence_number DESC
                LIMIT $2
                "#,
                conversation_id,
                limit
            )
            .fetch_all(&self.pool)
            .await?
        };

        Ok(messages)
    }

    pub async fn mark_delivered(&self, message_id: Uuid) -> AppResult<()> {
        sqlx::query!(
            r#"
            UPDATE messages
            SET delivered_at = NOW()
            WHERE id = $1 AND delivered_at IS NULL
            "#,
            message_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_read(&self, conversation_id: Uuid, user_id: Uuid) -> AppResult<()> {
        sqlx::query!(
            r#"
            UPDATE messages
            SET read_at = NOW()
            WHERE conversation_id = $1
              AND sender_id != $2
              AND read_at IS NULL
            "#,
            conversation_id,
            user_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

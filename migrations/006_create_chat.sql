-- Conversations and messages.
--
-- Design decisions:
-- 1. Conversations are uniquely identified by an ordered pair of participants.
--    CONSTRAINT conversations_participants_unique enforces this with a canonical
--    ordering (LEAST/GREATEST) applied in the application layer.
-- 2. Messages use a per-conversation sequence_number (generated via Redis INCR)
--    for ordering. This avoids a global sequence and scales horizontally.
-- 3. Idempotency key prevents duplicate messages on client retry.
--
-- Scaling: at large scale, messages are sharded by conversation_id (consistent
-- hashing) across a Cassandra cluster, giving O(1) writes and O(log n) reads
-- per conversation. PostgreSQL is used here for transactional integrity during
-- the initial scaling phase.

CREATE TYPE message_type AS ENUM ('text', 'image', 'video', 'file', 'system');

CREATE TABLE conversations (
    id               UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    -- participant_a < participant_b (lexicographic order on UUID string).
    -- Enforced by the application; the UNIQUE constraint guarantees dedup.
    participant_a    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    participant_b    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    last_message_id  UUID,
    last_message_at  TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT conversations_participants_unique UNIQUE (participant_a, participant_b),
    CONSTRAINT ordered_participants CHECK (participant_a < participant_b)
);

-- "List all conversations for user X, sorted by most recent activity" — inbox view.
CREATE INDEX conversations_participant_a ON conversations (participant_a, last_message_at DESC NULLS LAST);
CREATE INDEX conversations_participant_b ON conversations (participant_b, last_message_at DESC NULLS LAST);

CREATE TABLE messages (
    id               UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id  UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sender_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content          TEXT NOT NULL,
    message_type     message_type NOT NULL DEFAULT 'text',
    -- Monotonic sequence per conversation (Redis INCR). Enables cursor pagination
    -- and gap detection on the client ("did I miss any messages?").
    sequence_number  BIGINT NOT NULL,
    -- Client-provided UUID for exactly-once write semantics.
    -- ON CONFLICT (conversation_id, idempotency_key) DO NOTHING in the repo.
    idempotency_key  TEXT NOT NULL,
    delivered_at     TIMESTAMPTZ,
    read_at          TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (conversation_id, idempotency_key),
    -- sequence_number must be unique within a conversation (Redis INCR guarantees this).
    UNIQUE (conversation_id, sequence_number)
);

-- Primary read pattern: paginate messages in a conversation by sequence number.
CREATE INDEX messages_conv_seq ON messages (conversation_id, sequence_number DESC);
-- Unread count query: messages received by user X that haven't been read yet.
CREATE INDEX messages_unread  ON messages (conversation_id, sender_id, read_at)
    WHERE read_at IS NULL;

-- Foreign key to last_message_id (set after messages insert, so deferred).
ALTER TABLE conversations
    ADD CONSTRAINT fk_last_message
    FOREIGN KEY (last_message_id) REFERENCES messages(id)
    DEFERRABLE INITIALLY DEFERRED;

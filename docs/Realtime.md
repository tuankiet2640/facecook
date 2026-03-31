# Real-time System Design

## WebSocket Architecture

```
Client                     Chat Service Instance A           Redis          Instance B
  │                               │                            │                │
  │  WSS /api/v1/chat/ws         │                            │                │
  │  ?token=<jwt>                │                            │                │
  │──────────────────────────────►│                            │                │
  │                               │  validate JWT              │                │
  │                               │  register conn (DashMap)   │                │
  │                               │  set_presence_online()     │                │
  │                               │──── PUBLISH presence ─────►│                │
  │                               │                            │──── SUB msg ──►│
  │                               │                            │                │ push PresenceUpdate
  │                               │                            │                │ to B's connections
  │                               │                            │                │
  │  SendMessage{id, conv, text}  │                            │                │
  │──────────────────────────────►│                            │                │
  │                               │  1. check idempotency (NX) │                │
  │                               │  2. INCR seq:conv_id ─────►│                │
  │                               │  3. INSERT messages        │                │
  │                               │  4a. recipient online?     │                │
  │                               │      → DashMap lookup      │                │
  │                               │      → mpsc::send          │                │
  │                               │                            │                │
  │  Delivered{msg_id, seq}       │                            │                │
  │◄──────────────────────────────│                            │                │
  │                               │  4b. recipient offline?    │                │
  │                               │      → PUBLISH kafka       │                │
```

## Message Types (WsMessage enum)

All WebSocket frames are JSON with a `type` discriminant:

### Client → Server

**SendMessage** — Submit a new chat message.
```json
{
  "type": "send_message",
  "id": "client-generated-uuid",
  "conversation_id": "conv-uuid",
  "content": "Hello!",
  "message_type": "text"
}
```
The `id` field is the **idempotency key**. Clients generate a UUID v4 per message. On network retry, sending the same `id` again returns the original message — no duplicate is created.

**Ack** — Acknowledge receipt of a delivered message.
```json
{ "type": "ack", "message_id": "uuid" }
```
Triggers `UPDATE messages SET delivered_at = NOW()` for delivery receipts.

**Ping** — Keepalive. Send every 30 seconds.
```json
{ "type": "ping" }
```

### Server → Client

**NewMessage** — A message was received in one of your conversations.
```json
{
  "type": "new_message",
  "message": {
    "id": "uuid",
    "conversation_id": "uuid",
    "sender_id": "uuid",
    "content": "Hello!",
    "message_type": "text",
    "sequence_number": 42,
    "idempotency_key": "client-uuid",
    "created_at": "2026-03-31T10:00:00Z"
  }
}
```

**Delivered** — Confirmation that your sent message was persisted.
```json
{
  "type": "delivered",
  "message_id": "uuid",
  "sequence_number": 42
}
```

**PresenceUpdate** — A followed user came online or went offline.
```json
{
  "type": "presence_update",
  "user_id": "uuid",
  "online": true,
  "last_seen": "2026-03-31T10:05:00Z"
}
```

**Pong** — Response to client Ping.
```json
{ "type": "pong" }
```

**Error** — An operation failed.
```json
{
  "type": "error",
  "code": "SEND_FAILED",
  "message": "Rate limit exceeded"
}
```

---

## Delivery Guarantees

### At-Least-Once Delivery
- Messages are persisted to PostgreSQL **before** delivery is attempted.
- If delivery fails (network error, service crash), the message exists in the DB.
- Client reconnect path: on WebSocket connect, client calls `GET /api/v1/chat/conversations/:id/messages?before_sequence=<last_seen_seq>` to fetch missed messages.

### Idempotent Writes
- Client provides `idempotency_key` (UUID v4) per message.
- DB has `UNIQUE (conversation_id, idempotency_key)` — retried sends are silently deduplicated.
- Redis has `SET NX msg:idem:{conv_id}:{idem_key}` with 24h TTL — prevents double-processing before DB insert.

### Message Ordering
- `sequence_number` is generated via `INCR seq:{conversation_id}` in Redis.
- Monotonically increasing per conversation, no gaps under normal operation.
- Clients can detect gaps and request the missing range (retry with `before_sequence`).

### Offline Delivery
- When recipient is offline, a `message.queued` event is published to Kafka `chat.messages`.
- A **Notification Service** (not yet implemented) consumes this topic and sends mobile push notifications (APNs/FCM).
- On reconnect, client explicitly fetches message history — all messages are in the DB.

---

## Presence Design

### Protocol
1. WebSocket connect → `SET presence:{user_id} {online:true} EX 60` + `PUBLISH presence_changes`
2. WebSocket ping (every 30s) → `heartbeat()` refreshes TTL to 60s
3. WebSocket disconnect → `SET presence:{user_id} {online:false} EX 3600` + `PUBLISH presence_changes`
4. TTL expiry (missed heartbeat) → key disappears → presence queries return `online: false`

### Fan-out to Followers
- `PUBLISH presence_changes <PresenceEvent>` after every status change.
- Each chat-service instance runs a `PresenceSubscriber` background task that subscribes to this channel.
- On message receipt, the subscriber sends `WsMessage::PresenceUpdate` to all locally-connected WebSocket clients.
- Clients filter updates for users they follow (client-side).

### Scalability Note
The broadcast-to-all-connections approach works for small-to-medium deployments. At scale (>100K concurrent per instance), switch to a per-user sharded channel approach: subscribe only to `presence:{user_id}` channels for users that the local instance's connected clients follow.

---

## Connection Lifecycle

```
Connect → Authenticate → Register → [Send/Receive messages] → Disconnect → Cleanup
   │                         │                                       │
   │                         │                                       │
   ▼                         ▼                                       ▼
JWT validation         DashMap insert                      DashMap remove
                       presence online                     presence offline
                       pub/sub sub                         pub/sub unsub (auto)
```

**Reconnection strategy (client-side):**
1. Exponential backoff: 1s → 2s → 4s → ... → max 30s
2. Jitter to prevent thundering herd on service restart
3. On reconnect: fetch message history since `last_seen_sequence`

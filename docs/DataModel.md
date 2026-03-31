# Data Model

## Schema Overview

```
users ──────────┐
  │             │
  │ 1:N         │ M:N (follows)
  ▼             ▼
posts      follows (follower_id, followee_id)
  │
  │ via Kafka fanout
  ▼
celebrity_posts (fanout-on-read index)

users ──────────────── conversations ──────── messages
  │ (participant_a/b)        │
  │                          │ 1:N
  └──────────────────────────┘
```

## PostgreSQL Tables

### `users`
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK | v4 random, shard key |
| `username` | TEXT | UNIQUE (case-insensitive), GIN trigram index for search |
| `email` | TEXT | UNIQUE (case-insensitive) |
| `password_hash` | TEXT | Argon2id with random salt |
| `display_name` | TEXT | Nullable, user-facing name |
| `bio` | TEXT | Max 500 chars (enforced in app) |
| `avatar_url` | TEXT | Object storage URL (S3/GCS) |
| `follower_count` | BIGINT | Cached, maintained by trigger |
| `following_count` | BIGINT | Cached, maintained by trigger |
| `is_active` | BOOLEAN | Soft-disable without deleting |
| `created_at` | TIMESTAMPTZ | |
| `updated_at` | TIMESTAMPTZ | Auto-updated by trigger |

**Indexes:** username (unique), email (unique), username_trgm (GIN), display_name_trgm (GIN)

**Reasoning:** Cached counts avoid `COUNT(*)` on the hot read path. Trigram indexes enable `ILIKE '%query%'` searches efficiently.

---

### `follows`
| Column | Type | Notes |
|---|---|---|
| `follower_id` | UUID FK → users | |
| `followee_id` | UUID FK → users | |
| `created_at` | TIMESTAMPTZ | |

PK: `(follower_id, followee_id)`. CHECK constraint prevents self-follows.

**Indexes:**
- `(followee_id, created_at DESC)` — enumerate all followers of user X (fanout-on-write batch)
- `(follower_id, created_at DESC)` — enumerate all users that X follows (celebrity detection)

**Trigger:** `update_follow_counts` increments/decrements cached counts on users.

---

### `posts`
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK | |
| `author_id` | UUID FK → users | |
| `content` | TEXT | Max 50KB (enforced in app) |
| `media_urls` | JSONB | Array of object storage URLs |
| `tags` | TEXT[] | GIN indexed for `@>` queries |
| `like_count` | BIGINT | Cached count |
| `rank_score` | DOUBLE | Initially = epoch; extensible for ML re-ranking |
| `is_deleted` | BOOLEAN | Soft delete — post ID persists in feeds until TTL |
| `created_at` | TIMESTAMPTZ | |
| `updated_at` | TIMESTAMPTZ | |

**Indexes:**
- `(author_id, created_at DESC) WHERE NOT is_deleted` — celebrity fanout-on-read, profile view
- `(rank_score DESC) WHERE NOT is_deleted` — global trending (admin/explore)
- `tags GIN` — hashtag queries

---

### `celebrity_posts`
| Column | Type | Notes |
|---|---|---|
| `post_id` | UUID FK → posts | |
| `author_id` | UUID FK → users | |
| `timestamp_ms` | BIGINT | Redis sorted set score (epoch ms) |
| `created_at` | TIMESTAMPTZ | |

PK: `(author_id, post_id)`. This is the **durable fallback** for the Redis celebrity sorted set.

**Index:** `(author_id, timestamp_ms DESC)` — fetch recent celebrity posts on Redis cache miss.

---

### `conversations`
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK | |
| `participant_a` | UUID FK → users | `participant_a < participant_b` (canonical ordering) |
| `participant_b` | UUID FK → users | |
| `last_message_id` | UUID FK → messages | Nullable, used for inbox preview |
| `last_message_at` | TIMESTAMPTZ | Denormalized for sort-by-latest query |
| `created_at` | TIMESTAMPTZ | |

UNIQUE constraint on `(participant_a, participant_b)` ensures a single conversation per pair. Application normalizes ordering before insert.

---

### `messages`
| Column | Type | Notes |
|---|---|---|
| `id` | UUID PK | |
| `conversation_id` | UUID FK → conversations | |
| `sender_id` | UUID FK → users | |
| `content` | TEXT | Max 100KB |
| `message_type` | ENUM | text / image / video / file / system |
| `sequence_number` | BIGINT | Monotonic per-conversation (Redis INCR) |
| `idempotency_key` | TEXT | Client-provided UUID; ON CONFLICT skips duplicate |
| `delivered_at` | TIMESTAMPTZ | Set on recipient WebSocket ACK |
| `read_at` | TIMESTAMPTZ | Set on mark_read API call |
| `created_at` | TIMESTAMPTZ | |

UNIQUE constraints: `(conversation_id, idempotency_key)` and `(conversation_id, sequence_number)`.

**Indexes:**
- `(conversation_id, sequence_number DESC)` — paginated message fetch (cursor = sequence_number)
- `(conversation_id, sender_id, read_at) WHERE read_at IS NULL` — unread count

---

## Redis Data Structures

| Key Pattern | Type | TTL | Contents |
|---|---|---|---|
| `feed:{user_id}` | Sorted Set | 24h | Post UUIDs, score = timestamp_ms |
| `celebrity_posts:{author_id}` | Sorted Set | 24h | Post UUIDs, score = timestamp_ms |
| `seq:{conversation_id}` | String (counter) | none | Monotonic sequence number |
| `msg:idem:{conv_id}:{idem_key}` | String | 24h | Idempotency guard |
| `presence:{user_id}` | String (JSON) | 60s | `{user_id, online, last_seen}` |
| `rate:{user_id}` | Sorted Set | window | Request timestamps for sliding window |
| `jti:{jti}` | String | token TTL | Token revocation flag |

---

## SQL vs NoSQL Tradeoffs

| Concern | SQL (PostgreSQL) | NoSQL (Cassandra/Redis) |
|---|---|---|
| Transactional integrity | ✅ ACID | ❌ Eventual only |
| Schema flexibility | ❌ Migrations required | ✅ Schema-free |
| Query flexibility | ✅ Ad-hoc SQL | ❌ Primary key only |
| Write throughput | ❌ Limited by WAL | ✅ LSM-tree, high throughput |
| Read latency | ❌ Disk I/O on miss | ✅ In-memory (Redis) |
| Operational complexity | ✅ Simple | ❌ Complex tuning |

**Current choice:** PostgreSQL for all persistent state. Redis for hot paths. Move `messages` to Cassandra when write throughput exceeds PostgreSQL capacity (~10K msgs/sec per instance).

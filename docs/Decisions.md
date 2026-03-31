# Key Design Decisions

## 1. Hybrid Fanout Strategy

**Decision:** Fanout-on-write for regular users, fanout-on-read for celebrities.

**Context:** The "celebrity problem" — a user with 100M followers posting creates 100M Redis writes. Naive fanout-on-write becomes infeasible above ~10K followers.

**Alternatives considered:**
| Approach | Pros | Cons |
|---|---|---|
| Pure fanout-on-write | O(1) reads, simple | Write amplification for celebrities |
| Pure fanout-on-read | O(1) writes | Read latency scales with following count |
| **Hybrid (chosen)** | O(1) reads for most users, O(1) writes for celebrities | Complexity in build_feed merge |

**Implementation:** `FEED__CELEBRITY_THRESHOLD` env var (default 10K). Adjustable without code changes. The `build_feed` method merges personal feed (sorted set) with celebrity posts (separate sorted set) at read time, then sorts+deduplicates.

**Tradeoff accepted:** A user following 50 celebrities will have slightly higher feed read latency (~5ms extra for 50 Redis ZRANGE calls). This is acceptable vs. 10M Redis writes per celebrity post.

---

## 2. Database: PostgreSQL over Cassandra (initially)

**Decision:** Start with PostgreSQL for all persistent state.

**Rationale:**
- **ACID transactions** for critical paths (follow/unfollow with counter updates, message idempotency with ON CONFLICT).
- **Operational simplicity** — single database to operate during early scale.
- **SQL flexibility** — ad-hoc queries for debugging, backfills, and analytics without a separate ETL.
- **Schema evolution** — straightforward migrations via numbered SQL files.

**Migration path to Cassandra:** When `messages` table exceeds ~1 billion rows or write throughput exceeds ~5K msgs/sec, migrate `conversations` + `messages` to Cassandra. Schema: `PRIMARY KEY (conversation_id, sequence_number)` maps directly to Cassandra's partition+clustering key model.

---

## 3. Message Sequencing via Redis INCR

**Decision:** Use `INCR seq:{conversation_id}` in Redis for per-conversation sequence numbers.

**Alternatives:**
| Approach | Pros | Cons |
|---|---|---|
| PostgreSQL `SERIAL` / sequence | Durable, transactional | Requires DB round-trip on hot path |
| Snowflake ID (time+node+seq) | No coordination needed | Not per-conversation monotonic |
| **Redis INCR (chosen)** | O(1), in-memory speed | Loss on Redis restart without persistence |

**Durability:** Redis AOF persistence (`appendfsync everysec`) limits sequence loss to ≤1 second on crash. On restart, sequence resets, but `(conversation_id, idempotency_key) UNIQUE` in DB prevents true duplicates — only the sequence number is regenerated (not the message content).

---

## 4. WebSocket Auth via Query Parameter

**Decision:** JWT passed as `?token=<jwt>` in WebSocket URL, not in headers.

**Why:** The WebSocket HTTP upgrade handshake in browsers does not allow setting custom headers (unlike XMLHttpRequest / fetch). The only standard way to pass credentials is:
1. Query parameter (chosen) — visible in server logs, but encrypted in transit (TLS).
2. Cookie — requires same-origin; incompatible with cross-origin WS connections.
3. First message auth — requires holding open an unauthenticated connection briefly.

**Mitigation:** Tokens in URLs may appear in server access logs. Production deployment should ensure access logs are stored securely (not in Cloudflare/CDN access logs). Short-lived tokens (1h TTL) limit exposure window.

---

## 5. Event-Driven Fanout via Kafka

**Decision:** Post creation event → Kafka `post.events` → FanoutWorker.

**Why not synchronous fanout in post-service?**
- Synchronous fanout blocks the post creation HTTP response for O(N) Redis writes.
- If feed-service is down, post creation fails — violates the principle that core features should not depend on auxiliary features.

**Kafka guarantees:**
- **At-least-once delivery:** FanoutWorker commits offsets only after successful processing. On crash, replay from last committed offset.
- **Idempotency:** Feed sorted set `ZADD` is idempotent (same score + member = no-op). Re-processing a `PostCreated` event is safe.
- **Ordering:** Posts by the same author go to the same Kafka partition (keyed by `author_id`), preserving per-author ordering in the fanout worker.

---

## 6. Stateless Services

**Decision:** All service instances are stateless. Shared state lives in PostgreSQL/Redis/Kafka.

**Consequence for chat:** WebSocket connections are stateful (in-memory DashMap). This means a message sent to instance A cannot be directly delivered to a recipient connected to instance B.

**Solution:** Redis pub/sub bridges instances. When instance A wants to deliver a message, it checks its local DashMap. If recipient is not local, the message is in Kafka — the recipient's instance will deliver it via its own Kafka consumer (offline delivery) or the recipient will fetch it on reconnect.

**Alternative not chosen:** Sticky load balancing (route user X always to instance X). This was rejected because it creates hot spots and complicates rolling deployments.

---

## 7. Soft Delete for Posts

**Decision:** Posts use `is_deleted = true` flag, never physical DELETE.

**Rationale:**
- Feeds contain post UUIDs in Redis sorted sets. Physical deletion would leave dangling references that trigger 404s on feed hydration.
- Soft delete allows the fanout worker to clean up feeds asynchronously without requiring a distributed transaction.
- Audit compliance — regulators may require records of deleted content.

**Tradeoff:** Storage grows over time. Periodic archival job moves old soft-deleted posts to cold storage (S3 Glacier).

---

## 8. Eventual Consistency Model

**Consistent (immediate):**
- Authentication (JWT validation, issuance)
- Message persistence (DB insert is synchronous)
- User creation/update

**Eventually consistent (async):**
- Feed updates after a new post (Kafka latency: typically <1s)
- Follower/following counts (trigger-based, sub-millisecond but not transactional with follow)
- Presence state (TTL-based: up to 60s stale)

**Accepted staleness budget:**
- Feed: <2 seconds from post creation to feed appearance
- Presence: <60 seconds (TTL expiry + heartbeat interval)
- Follow counts: <100ms (trigger on same transaction)

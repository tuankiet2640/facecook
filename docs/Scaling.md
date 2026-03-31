# Scaling to 10M Users

## Current Bottlenecks and Solutions

### 1. Feed Fanout (Write Amplification)

**Problem:** A user with 10M followers posting creates 10M Redis writes in a tight loop — this saturates network I/O and Redis CPU on a single instance.

**Solution:** Hybrid fanout strategy.
- Users with `≤ celebrity_threshold` (default 10K) followers: **fanout-on-write**. Post is pushed to all follower feed sorted sets concurrently in batches of 1,000. At 10K followers, that's 10 Kafka-consumed, Redis-pipelined batches — sub-second.
- Users with `> celebrity_threshold` followers: **fanout-on-read**. Post is stored in `celebrity_posts:{author_id}` sorted set. At read time, the user's personal feed is merged with celebrity posts inline. Read complexity is O(P + C×K) where P=personal feed size, C=celebrities followed (typically <10), K=posts per celebrity (~20).

**Scale path:** Shard Kafka `post.events` by `author_id` (same partition = same fanout worker). Add more feed-service replicas, each consuming a partition subset. Redis cluster shards by `feed:{user_id}` key hash.

### 2. Database Connections

**Problem:** PostgreSQL max_connections ≈ 200. At 10 service instances × 20 connections = 200 — already saturated.

**Solution:**
- Use PgBouncer (transaction pooling) in front of PostgreSQL. Each service connects to PgBouncer which multiplexes into 10–20 actual DB connections.
- Read replicas for heavy read paths (feed builds on cache miss, user profile lookups).
- Feed sorted sets in Redis absorb 99% of feed reads — DB is only hit on cold cache miss.

### 3. Chat Message Storage

**Problem:** PostgreSQL with a single `messages` table becomes a hotspot at billions of rows.

**Solution:** Migrate messages to Apache Cassandra (or Amazon DynamoDB).
- Partition key: `conversation_id` — all messages for a conversation on the same node.
- Clustering key: `sequence_number DESC` — efficient range scans for pagination.
- Cassandra's LSM-tree storage optimizes for high-throughput sequential writes.
- Keep PostgreSQL for `conversations` metadata (small, transactional).

### 4. WebSocket Connection State

**Problem:** A single chat-service instance can hold ~50K WebSocket connections (limited by file descriptors and memory). At 1M concurrent users, that's 20 instances.

**Solution:**
- Sticky routing: load balancer (e.g., Nginx upstream hash on user_id) routes a user's WebSocket to the same instance.
- Redis pub/sub fan-out: when instance A wants to deliver to a user on instance B, it publishes to Redis. Instance B's subscriber picks it up. This is the current `presence_changes` channel pattern, extended to chat messages.
- At extreme scale (>1M concurrent), replace Redis pub/sub with a message bus (e.g., NATS JetStream) that has better fan-out semantics.

### 5. User Service Follow Graph

**Problem:** `SELECT * FROM follows WHERE followee_id = $1` scans millions of rows for celebrities.

**Solution:**
- Batch follower queries (already implemented, 1K/batch).
- Cache follower list pages in Redis with TTL.
- At extreme scale (Twitter-scale), move the social graph to a dedicated graph store (Neo4j, Amazon Neptune, or a custom adjacency list in Cassandra).

## Horizontal Scaling Plan

| Users | PostgreSQL | Redis | Kafka | Services |
|---|---|---|---|---|
| 100K | Single instance | Single instance | 3 brokers | 1 replica each |
| 1M | Primary + 1 read replica, PgBouncer | Sentinel (3 nodes) | 6 brokers, 12 partitions | 3–5 replicas |
| 10M | Sharded (Citus or Vitess), PgBouncer | Cluster (6 shards) | 12 brokers, 24 partitions | 10–20 replicas, HPA |
| 100M | Cassandra for messages, Postgres for metadata | Cluster | 24+ brokers | Auto-scaled, multi-region |

## Caching Strategy

```
Client request
    │
    ▼
Redis feed sorted set ──hit──► return cached feed items
    │
    │ miss
    ▼
PostgreSQL celebrity_posts table ──► rebuild Redis cache ──► return
```

- **Feed sorted set hit rate target:** >95% (achieved by keeping TTL = 24h and max_feed_size = 1000)
- **Post content:** not cached in feed service (fetched from post-service or its cache by the client)
- **Presence:** Redis key with 60s TTL; heartbeat refresh every 30s

## Rate Limiting

Implemented as a sliding window counter in Redis (sorted set of timestamps):
- Default: 100 requests per 60 seconds per user
- Post creation: 20 posts per minute (configurable)
- Message send: 60 messages per minute per conversation

## SLA Target

| Metric | Target | Mechanism |
|---|---|---|
| Feed read p99 latency | <200ms | Redis sorted set merge, no DB on hot path |
| Message delivery (online) | <50ms | In-process DashMap lookup + mpsc send |
| Message delivery (offline) | <5s | Kafka consumer lag target |
| Service availability | 99.99% | N+1 replicas, health checks, circuit breakers |
| Feed write latency | <500ms | Async Kafka fanout, non-blocking to publisher |

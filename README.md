# Facecook

A production-grade distributed backend combining a **news feed** (à la Twitter/Facebook) and **real-time chat** (à la Messenger/WhatsApp). Built in Rust as a demonstration of distributed system
```

---

## What's inside

| Service | Port | Responsibility |
|---|---|---|
| **ui** | 3001 | React + TypeScript frontend |
| **gateway** | 8080 | JWT auth, rate limiting, HTTP reverse proxy |
| **user-service** | 8081 | Registration, login, follow graph, profiles |
| **post-service** | 8083 | Post CRUD, Kafka event publishing |
| **feed-service** | 8082 | Personalized feed with hybrid fanout |
| **chat-service** | 8084 | WebSocket messaging, presence management |
| **presence-service** | 8085 | Online/offline status API |

**Backing services:** PostgreSQL · Redis · Apache Kafka

**Observability:** Prometheus (`:9100`) · Grafana (`:3000`)

---

## Architecture

```
                         ┌─────────────────────────┐
                         │      API Gateway :8080   │
                         │  JWT · Rate limit · Proxy│
                         └──┬──────┬──────┬────┬───┘
                            │      │      │    │
               ┌────────────┘  ┌───┘  ┌───┘  └────────────┐
               ▼               ▼      ▼                     ▼
        ┌──────────┐  ┌──────────┐ ┌──────────┐  ┌──────────────────┐
        │  Users   │  │  Posts   │ │   Feed   │  │  Chat + Presence │
        │  :8081   │  │  :8083   │ │  :8082   │  │  :8084 / :8085   │
        └────┬─────┘  └────┬─────┘ └────┬─────┘  └────────┬─────────┘
             │             │             │                  │
             └─────────────┴─────────────┴──────────────────┘
                                         │
                  ┌──────────────────────┼───────────────────────┐
                  │                      │                        │
           ┌──────────┐          ┌───────────────┐        ┌──────────┐
           │PostgreSQL│          │     Redis      │        │  Kafka   │
           │          │          │ feed sets      │        │          │
           │ users    │          │ presence TTLs  │        │post.evts │
           │ posts    │          │ seq numbers    │        │chat.msgs │
           │ messages │          │ rate limits    │        │notifs    │
           └──────────┘          │ pub/sub        │        └──────────┘
                                 └───────────────┘
```

### Feed — Hybrid Fanout

The core algorithmic challenge is the **celebrity problem**: a user with 10M followers posting creates 10M Redis writes if you naively fan out to every follower.

**Solution:** fanout strategy switches based on follower count at write time.

```
Post created
    │
    ▼ (Kafka: post.events)
FanoutWorker
    │
    ├── followers ≤ 10K ──► FANOUT-ON-WRITE
    │                        Push post_id to every follower's
    │                        Redis sorted set (batches of 1000)
    │                        Read: O(1) — pre-computed
    │
    └── followers > 10K ──► FANOUT-ON-READ
                             Index post in celebrity_posts:{author_id}
                             Write: O(1)
                             Read: merge personal feed + celebrity posts
```

The threshold (`FEED__CELEBRITY_THRESHOLD`) is a runtime env var — no deploy needed to tune it.

### Chat — Message Delivery

```
Client → WebSocket → ChatService
                         │
                         ├─ 1. SET NX idempotency key (Redis)
                         ├─ 2. INCR seq:{conv_id} (Redis) → sequence number
                         ├─ 3. INSERT messages (PostgreSQL, ON CONFLICT IGNORE)
                         │
                         ├─ recipient online? ──► mpsc::send → WebSocket
                         └─ recipient offline? ──► PUBLISH kafka → push notification
```

No message is ever lost: persistence happens before delivery is attempted. Clients re-fetch history on reconnect.

---

## Quick start

**Prerequisites:** Docker + Docker Compose v2

```bash
# Clone and enter
cd facecook

# Copy env template (review JWT_SECRET before running)
cp .env.example .env

# Start everything
docker compose up -d

# Wait for health checks (~30s first run)
docker compose ps

# Smoke test
curl http://localhost:8080/health
# {"status":"healthy","timestamp":"..."}
```

**Ports after startup:**

| URL | What |
|---|---|
| **http://localhost:3001** | **UI (open this in your browser)** |
| http://localhost:8080 | API gateway (REST) |
| http://localhost:3000 | Grafana (admin / admin) |
| http://localhost:9100 | Prometheus |
| http://localhost:8090 | Kafka UI (run with `--profile tools`) |

---

## Usage examples

### Register and get a token
```bash
curl -s -X POST http://localhost:8080/api/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"alice","email":"alice@example.com","password":"secret123"}' \
  | jq .access_token
```

### Follow someone and create a post
```bash
TOKEN="eyJhbGci..."

# Follow user
curl -X POST http://localhost:8080/api/v1/users/<user_id>/follow \
  -H "Authorization: Bearer $TOKEN"

# Create a post
curl -X POST http://localhost:8080/api/v1/posts \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"Hello distributed world! #rust","tags":["rust"]}'
```

### Read your feed
```bash
curl http://localhost:8080/api/v1/feed?limit=20 \
  -H "Authorization: Bearer $TOKEN"
```

### Connect to chat via WebSocket
```bash
# Using websocat (https://github.com/vi/websocat)
websocat "ws://localhost:8084/api/v1/chat/ws?token=$TOKEN"

# Send a message
{"type":"send_message","id":"$(uuidgen)","conversation_id":"<conv_uuid>","content":"hey","message_type":"text"}
```

---

## UI — local development (without Docker)

```bash
cd ui
npm install
npm run dev          # http://localhost:5173

# Vite proxies /api → http://localhost:8080 (gateway)
# and /ws → ws://localhost:8084 (chat-service)
# so the backend stack must be running separately.
```

## Project structure

```
facecook/
├── Cargo.toml                    # Workspace root
├── Dockerfile                    # Multi-stage build (cargo-chef for layer caching)
├── docker-compose.yml            # Full local stack
├── .env.example                  # All required environment variables
│
├── ui/                           # React + TypeScript frontend
│   ├── src/
│   │   ├── App.tsx               # Routes: /auth /feed /chat/:id /profile/:id
│   │   ├── types/                # api.ts (all API shapes) · ws.ts (WsMessage union)
│   │   ├── lib/                  # api.ts (Axios + interceptors) · queryKeys.ts · utils.ts
│   │   ├── stores/auth.ts        # Zustand: token, userId, user — localStorage synced
│   │   ├── hooks/
│   │   │   ├── useChat.ts        # WebSocket with exponential backoff + missed msg recovery
│   │   │   ├── useFeed.ts        # Infinite scroll + batch post hydration
│   │   │   └── usePresence.ts    # Polling presence for chat participants
│   │   ├── pages/                # AuthPage · FeedPage · ChatPage · ProfilePage
│   │   └── components/           # AppShell · Sidebar · PostCard · MessageBubble
│   ├── Dockerfile                # Node build → nginx:alpine serve
│   └── nginx.conf                # Proxy /api → gateway, /ws → chat, SPA fallback
│
├── crates/
│   ├── shared/                   # Shared library (auth, cache, db, kafka, models)
│   │   └── src/
│   │       ├── auth.rs           # JWT service (issue + validate)
│   │       ├── cache.rs          # Redis client (typed get/set, zadd, pub/sub)
│   │       ├── config.rs         # AppConfig (loaded from env via config crate)
│   │       ├── db.rs             # PostgreSQL pool factory
│   │       ├── errors.rs         # AppError enum + IntoResponse
│   │       ├── kafka.rs          # KafkaProducer + create_consumer
│   │       ├── observability.rs  # init_tracing, init_metrics, health_check
│   │       └── models/           # Shared domain types (User, Post, Message, Feed)
│   │
│   ├── gateway/                  # API Gateway — auth, rate limiting, HTTP proxy
│   ├── user-service/             # Users, auth, follow graph
│   ├── post-service/             # Post CRUD + Kafka event publishing
│   ├── feed-service/             # Feed build + hybrid fanout worker
│   ├── chat-service/             # WebSocket chat + presence integration
│   └── presence-service/         # Online/offline status API
│
├── migrations/
│   ├── 001_create_extensions.sql
│   ├── 002_create_users.sql      # users table + triggers + GIN indexes
│   ├── 003_create_follows.sql    # follow graph + cached count triggers
│   ├── 004_create_posts.sql      # posts + rank_score + GIN tag index
│   ├── 005_create_feed.sql       # celebrity_posts durability table
│   └── 006_create_chat.sql       # conversations + messages + idempotency
│
├── infra/
│   ├── prometheus/prometheus.yml # Scrape config for all 6 services
│   └── grafana/datasources/      # Auto-provisioned Prometheus data source
│
└── docs/
    ├── Architecture.md           # System diagram + component breakdown
    ├── Scaling.md                # Bottlenecks + path to 10M users
    ├── DataModel.md              # Full schema + Redis key patterns
    ├── API.md                    # REST + WebSocket API reference
    ├── Realtime.md               # WebSocket protocol + delivery guarantees
    ├── Decisions.md              # 8 ADRs with explicit tradeoffs
    └── 12factor.md               # 12-Factor compliance evidence
```

---

## Configuration

All config is via environment variables. No defaults are hardcoded — missing required vars cause an immediate startup panic (fail fast, not silently broken).

```bash
# Key variables (see .env.example for the complete list)

DATABASE__URL=postgresql://facecook:facecook@postgres:5432/facecook
REDIS__URL=redis://redis:6379
KAFKA__BROKERS=kafka:9092

AUTH__JWT_SECRET=<openssl rand -hex 32>

FEED__CELEBRITY_THRESHOLD=10000   # Fanout strategy cutoff
FEED__MAX_FEED_SIZE=1000          # Redis sorted set max entries per user

REDIS__PRESENCE_TTL_SECS=60       # Heartbeat must arrive within this window
```

Double-underscore maps to nested config: `FEED__CELEBRITY_THRESHOLD` → `AppConfig.feed.celebrity_threshold`.

---

## Observability

### Metrics (Prometheus)
Every service exposes `/metrics` on port 9090. Pre-registered metrics:

| Metric | Type | Description |
|---|---|---|
| `http_requests_total` | Counter | Request count by status |
| `http_request_duration_seconds` | Histogram | Latency distribution |
| `feed_fanout_total{strategy}` | Counter | write vs read fanout counts |
| `feed_fanout_duration_seconds` | Histogram | Time per fanout operation |
| `messages_sent_total` | Counter | Messages submitted by clients |
| `messages_delivered_total{method}` | Counter | websocket / kafka_queued |
| `websocket_connections_active` | Gauge | Live WS connections per instance |
| `kafka_events_consumed_total{status}` | Counter | success / error / malformed |
| `rate_limit_rejections_total` | Counter | Rate limiter hits |

### Logs (structured JSON in production)
```json
{
  "timestamp": "2026-03-31T10:00:00Z",
  "level": "INFO",
  "target": "feed_service::workers::fanout_worker",
  "message": "Fanout batch written",
  "post_id": "abc123",
  "batch_size": 1000,
  "offset": 0,
  "total_followers": 5000
}
```

Set `RUST_LOG=debug` for verbose output. `RUST_LOG=info,sqlx=warn` is recommended for normal operation.

### Health checks
All services expose `GET /health → {"status":"healthy","timestamp":"..."}`. Used by Docker Compose healthchecks, load balancers, and Kubernetes liveness probes.

---

## Key design decisions

| Decision | Choice | Why |
|---|---|---|
| Feed fanout | Hybrid write+read | Avoids write amplification for high-follower users |
| Message sequencing | Redis INCR per conv | O(1), no DB lock; loss-tolerant via idempotency key |
| Message idempotency | `(conv_id, idem_key) UNIQUE` + Redis NX | Exactly-once writes with at-least-once transport |
| Database | PostgreSQL | ACID for critical paths; Cassandra migration path documented |
| WebSocket auth | JWT in `?token=` | Browsers cannot set headers on WS upgrade |
| Presence | Redis TTL + heartbeat | Simple, correct, sub-second staleness |
| Gateway proxy | reqwest | Observable, configurable; replace with Envoy at scale |
| Event bus | Kafka | At-least-once, ordered per partition, replay on crash |

Full reasoning in [docs/Decisions.md](docs/Decisions.md).

---

## Scaling

| Users | PostgreSQL | Redis | Kafka | Replicas |
|---|---|---|---|---|
| 100K | Single + PgBouncer | Single | 3 brokers | 1 each |
| 1M | Primary + replica | Sentinel | 6 brokers | 3–5 each |
| 10M | Citus sharding | Cluster | 12 brokers | 10–20 + HPA |
| 100M | Cassandra for messages | Cluster | 24+ brokers | Multi-region |

See [docs/Scaling.md](docs/Scaling.md) for the full breakdown.

---

## Tech stack

| Layer | Technology |
|---|---|
| Language | Rust 1.88 |
| Web framework | Axum 0.7 + Tokio |
| Database ORM | sqlx 0.7 (compile-time checked queries) |
| Cache | Redis 7 via deadpool-redis |
| Message bus | Apache Kafka 3.7 via rdkafka |
| Auth | JWT HS256 (jsonwebtoken 9) + Argon2id |
| Metrics | Prometheus via metrics-exporter-prometheus |
| Tracing | tracing + tracing-subscriber (JSON/pretty) |
| HTTP proxy | reqwest 0.12 |
| Build cache | cargo-chef |
| Container | Docker (multi-stage, Debian Slim runtime) |

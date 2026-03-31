# Architecture

## High-Level Diagram

```
                           ┌────────────────────────────────────────────────┐
                           │                  CLIENTS                        │
                           │   Web / iOS / Android / CLI                     │
                           └──────────────────┬─────────────────────────────┘
                                              │ HTTPS / WSS
                                              ▼
                           ┌────────────────────────────────────────────────┐
                           │               API GATEWAY :8080                 │
                           │  • JWT validation (HS256)                       │
                           │  • Sliding-window rate limiting (Redis)         │
                           │  • Header injection (X-User-Id, X-Username)    │
                           │  • HTTP reverse proxy (reqwest)                 │
                           │  • Metrics / tracing                            │
                           └───┬────────┬────────┬────────┬────────┬────────┘
                               │        │        │        │        │
            ┌──────────────────┘        │        │        │        └──────────────────┐
            ▼                           ▼        ▼        ▼                           ▼
  ┌──────────────────┐    ┌──────────────┐  ┌──────────┐  ┌──────────────┐  ┌──────────────────┐
  │  User Service    │    │ Post Service │  │  Feed    │  │    Chat      │  │ Presence Service │
  │  :8081           │    │  :8083       │  │ Service  │  │  Service     │  │  :8085           │
  │                  │    │              │  │  :8082   │  │  :8084       │  │                  │
  │  register/login  │    │  CRUD posts  │  │          │  │  WebSocket   │  │  online/offline  │
  │  follow graph    │    │  media meta  │  │  fanout  │  │  1:1 chat    │  │  heartbeat       │
  │  profiles        │    │  pub to      │  │  feed    │  │  msg persist │  │  batch query     │
  │  JWT issuance    │    │  Kafka       │  │  build   │  │  delivery    │  │                  │
  └────────┬─────────┘    └──────┬───────┘  └────┬─────┘  └──────┬───────┘  └────────┬─────────┘
           │                     │               │               │                    │
           └─────────────────────┴───────────────┴───────────────┴────────────────────┘
                                              │
                     ┌────────────────────────┴──────────────────────────┐
                     │                 BACKING SERVICES                   │
                     │                                                    │
                     │  ┌─────────────┐  ┌──────────────┐  ┌──────────┐ │
                     │  │ PostgreSQL  │  │    Redis     │  │  Kafka   │ │
                     │  │             │  │              │  │          │ │
                     │  │ users       │  │ feed sorted  │  │ post.    │ │
                     │  │ follows     │  │ sets         │  │ events   │ │
                     │  │ posts       │  │ rate limits  │  │          │ │
                     │  │ celebrity   │  │ presence     │  │ chat.    │ │
                     │  │ posts       │  │ sessions     │  │ messages │ │
                     │  │ convs /     │  │ seq numbers  │  │          │ │
                     │  │ messages    │  │ pub/sub      │  │ notifs   │ │
                     │  └─────────────┘  └──────────────┘  └──────────┘ │
                     └────────────────────────────────────────────────────┘
```

## Components

### API Gateway (port 8080)
Single ingress point. Validates JWTs and proxies authenticated requests downstream. Strips `Authorization` headers and injects `X-User-Id` / `X-Username` — downstream services trust these internal headers without re-validating the JWT. Rate limiting is enforced here before any service is hit.

### User Service (port 8081)
Owns authentication (register, login, JWT issuance) and the social graph (follow/unfollow). Publishes `user.events` to Kafka when follow relationships change so other services can react (e.g., feed service can update celebrity threshold checks).

### Post Service (port 8083)
Owns post lifecycle (create, soft-delete, retrieve). On post creation, publishes a `PostCreated` event to the `post.events` Kafka topic. The feed service's fanout worker consumes this to trigger feed distribution.

### Feed Service (port 8082)
Owns feed generation using a **hybrid fanout strategy** (see Decisions.md). Maintains Redis sorted sets per user. Background Kafka consumer (`FanoutWorker`) processes `PostCreated` events asynchronously so post creation is non-blocking.

### Chat Service (port 8084)
Owns 1:1 messaging. Clients connect via WebSocket (`/api/v1/chat/ws?token=<jwt>`). The service:
1. Authenticates the WebSocket via JWT in the query string.
2. Registers the connection in an in-memory `DashMap<Uuid, mpsc::Sender>`.
3. Attempts direct delivery to the recipient if online.
4. Falls back to Kafka `chat.messages` topic for offline delivery / push notifications.
5. Subscribes to Redis pub/sub `presence_changes` channel to push real-time presence updates to connected clients.

### Presence Service (port 8085)
Stateless HTTP API backed by Redis. Each user's online status is stored as a Redis key with a 60-second TTL. Clients must heartbeat every 30 seconds to stay "online". Presence changes are published to Redis pub/sub so chat-service instances can push `PresenceUpdate` WsMessages to connected followers.

## Tradeoffs

| Decision | Choice | Rationale |
|---|---|---|
| Service isolation | 6 microservices | Clear domain ownership; independent scale and deploy |
| Feed fanout | Hybrid write+read | Avoids write amplification for celebrities; see Decisions.md |
| WebSocket auth | JWT in query param | Browsers cannot set custom headers on WebSocket upgrade |
| Message ordering | Redis INCR per conversation | O(1) sequence generation without DB lock contention |
| Presence TTL | 60s with heartbeat | Balances accuracy vs Redis write load |
| Gateway proxy | reqwest HTTP proxy | Simple and observable; production would use Envoy/Nginx |

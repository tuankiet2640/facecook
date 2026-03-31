# 12-Factor App Compliance

## I. Codebase — One codebase, many deploys ✅

Single Git repository (`facecook/`) contains all services as a Cargo workspace. Each service is a separate binary (`[[bin]]` in Cargo.toml) sharing the `shared` library crate.

Deployed to dev / staging / production by varying environment variables — the code is identical across environments.

```
facecook/
├── Cargo.toml          # workspace root
├── crates/
│   ├── shared/         # shared library
│   ├── gateway/        # binary: gateway
│   ├── user-service/   # binary: user-service
│   └── ...
```

---

## II. Dependencies — Explicitly declare and isolate ✅

All dependencies are declared in `Cargo.toml` (workspace-level with per-crate overrides). `Cargo.lock` pins exact versions for reproducible builds.

No implicit system-level dependencies — the Dockerfile installs only `libssl3` and `ca-certificates` at runtime, which are standard and versioned.

---

## III. Config — Store config in the environment ✅

Zero configuration is hardcoded. All values are loaded from environment variables at startup via the `config` crate with `__` separator:

```bash
DATABASE__URL=postgresql://...
AUTH__JWT_SECRET=...
FEED__CELEBRITY_THRESHOLD=10000
```

Default values are intentionally not provided in code — missing required config causes a panic at startup (fail fast). See `.env.example` for the full list.

Config is validated at startup via `AppConfig::load()` which returns `ConfigError` for missing or malformed values — no config surprises at runtime.

---

## IV. Backing Services — Treat as attached resources ✅

PostgreSQL, Redis, and Kafka are all referenced by URL/connection string in environment variables. Switching from local PostgreSQL to Amazon RDS requires only changing `DATABASE__URL` — no code changes.

The gateway's HTTP client (reqwest) treats downstream services as attached resources referenced by `http://user-service:8081` — resolvable via Docker DNS or Kubernetes service discovery.

---

## V. Build, Release, Run — Strict separation ✅

```
Build stage (Dockerfile):
  cargo build --release --bin $SERVICE
  → produces /app/target/release/$SERVICE

Release stage (docker-compose / Kubernetes manifest):
  Runtime image = build artifact + environment config
  → immutable; config is injected at runtime, not baked in

Run stage:
  ENTRYPOINT ["/app/service"]
  → reads config from env, connects to backing services, serves traffic
```

The multi-stage Dockerfile separates compilation from the runtime image. The runtime image is ~50MB (Debian slim + binary + libssl3) vs ~5GB build image.

---

## VI. Processes — Execute as stateless processes ✅

All application state is externalized:
- **Sessions:** JWT tokens (stateless) + Redis JTI revocation set
- **Feed:** Redis sorted sets (ephemeral cache) + PostgreSQL (durable)
- **Chat connections:** in-process DashMap (session-scoped, expected to be lost on restart)
- **Message state:** PostgreSQL (the source of truth)

WebSocket connections are session-local (stateful within a process) — this is unavoidable for WebSocket servers. Clients reconnect and re-fetch state from the DB on disconnect (see Realtime.md).

---

## VII. Port Binding — Export services via port binding ✅

Each service binds to `$SERVER__HOST:$SERVER__PORT`. Services are self-contained HTTP/WebSocket servers (using Tokio + Axum) — no external web server (Apache/Nginx) is required.

Prometheus metrics are exported on port 9090 (separate from the application port) via `metrics-exporter-prometheus`.

---

## VIII. Concurrency — Scale out via the process model ✅

Services scale horizontally by adding replicas. Docker Compose `scale` or Kubernetes HPA adds more instances. Each instance is independent — they compete for Kafka partition assignments and share Redis/Postgres as backing stores.

Background workers (FanoutWorker, PresenceSubscriber) run as Tokio tasks within the service process — not as separate processes. Kafka consumer groups handle partition balancing across replicas automatically.

---

## IX. Disposability — Maximize robustness with fast startup/shutdown ✅

**Fast startup:**
Startup time is <500ms (compile-time binary, no JVM warmup). Config validation happens first — the process exits immediately if required env vars are missing.

**Graceful shutdown:**
`axum::serve` handles `SIGTERM` by draining in-flight requests before shutdown. Kafka consumers commit their last offset on drop (via rdkafka's auto-commit on `Drop`).

**Crash recovery:**
Workers catch panics and restart with exponential backoff:
```rust
loop {
    if let Err(e) = worker.run().await {
        tracing::error!(error = %e, "Worker crashed — restarting in 5s");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
```

---

## X. Dev/Prod Parity — Keep development and production as similar as possible ✅

`docker-compose.yml` runs the exact same Docker image, Kafka version, PostgreSQL version, and Redis version as production. The only differences are:

| Factor | Dev | Prod |
|---|---|---|
| `ENVIRONMENT` | `development` | `production` |
| Log format | `pretty` (human-readable) | `json` (structured, machine-readable) |
| Secret management | `.env` file | AWS Secrets Manager / Vault |
| Replicas | 1 each | N (HPA-managed) |

---

## XI. Logs — Treat logs as event streams ✅

Services write to **stdout only** (no log files, no log rotation). `tracing-subscriber` formats:
- `development`: pretty-printed with colors
- `production`: newline-delimited JSON (structlog format) for ingestion by Datadog/ELK/CloudWatch

```json
{"timestamp":"2026-03-31T10:00:00Z","level":"INFO","target":"feed_service::workers::fanout_worker","message":"Fanout batch written","post_id":"abc123","batch_size":1000,"offset":0}
```

Log level controlled by `RUST_LOG` env var (`info`, `debug`, `trace`, etc.) at runtime without redeployment.

---

## XII. Admin Processes — Run admin/management tasks as one-off processes ✅

Database migrations are SQL files in `migrations/` loaded by PostgreSQL's `docker-entrypoint-initdb.d` mechanism on first start. In production, run migrations as a Kubernetes `Job` before rolling out new service versions:

```bash
# Apply pending migrations
psql $DATABASE__URL -f migrations/006_create_chat.sql

# Health check before routing traffic
curl http://user-service:8081/health
```

Seed data, backfill scripts, and data migrations are separate one-off binaries or SQL scripts — not bundled into the application binary.

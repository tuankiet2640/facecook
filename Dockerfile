# ── Stage 1: Build ────────────────────────────────────────────────────────────
#
# Uses cargo-chef for layer-cached dependency compilation.
# Dependencies are compiled in a separate layer so source changes don't
# invalidate the dependency cache — critical for <60s CI rebuild times.
#
# ARG SERVICE controls which binary is built (e.g. gateway, user-service).
FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Stage 2: Planner (dependency recipe) ──────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: Builder ──────────────────────────────────────────────────────────
FROM chef AS builder

ARG SERVICE
# Validate SERVICE arg is provided
RUN test -n "$SERVICE" || (echo "ERROR: SERVICE build arg is required" && exit 1)

# Install build dependencies for rdkafka (requires cmake + libssl).
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Restore cached dependencies layer (only invalidated when Cargo.toml changes).
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build the target service binary.
# SQLX_OFFLINE=true uses pre-generated .sqlx query cache instead of a live DB.
# Generate the cache locally with: cargo sqlx prepare --workspace
ENV SQLX_OFFLINE=true
COPY . .
RUN cargo build --release --bin "$SERVICE"

# ── Stage 4: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

ARG SERVICE
ENV SERVICE=$SERVICE

# Runtime dependencies: libssl for TLS, ca-certificates for HTTPS.
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security — process cannot write to system directories.
RUN useradd --uid 10001 --no-create-home --shell /sbin/nologin app
USER app

WORKDIR /app

# Copy only the compiled binary from the builder stage.
COPY --from=builder /app/target/release/$SERVICE /app/service

# Prometheus metrics scrape port.
EXPOSE 9090

# Health check — all services expose /health.
# Adjust port via SERVER__PORT env var in docker-compose.
HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD ["/bin/sh", "-c", "wget -qO- http://localhost:${SERVER__PORT:-8080}/health | grep -q healthy"]

ENTRYPOINT ["/app/service"]

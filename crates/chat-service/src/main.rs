use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use dashmap::DashMap;
use tokio::{net::TcpListener, sync::mpsc};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::info;

use shared::{
    auth::JwtService,
    cache::{create_redis_pool, CacheClient},
    config::AppConfig,
    db::create_pool,
    kafka::KafkaProducer,
    models::message::WsMessage,
    observability::{init_metrics, init_tracing},
};

mod api;
mod domain;
mod repository;
mod service;
mod workers;

use repository::message_repo::MessageRepository;
use service::chat_service::ChatService;
use workers::presence_subscriber::PresenceSubscriber;

/// Per-connection sender. Each WebSocket connection registers its unbounded
/// channel here so the service layer can push messages without holding locks.
pub type ConnTx = mpsc::UnboundedSender<WsMessage>;

/// Thread-safe map from user_id → sender for active WebSocket connections.
/// DashMap gives fine-grained per-shard locking — safe to clone the Arc.
pub type ConnectionRegistry = Arc<DashMap<uuid::Uuid, ConnTx>>;

pub struct AppState {
    pub chat_service: Arc<ChatService>,
    pub connections: ConnectionRegistry,
    pub jwt_service: Arc<JwtService>,
    pub config: Arc<AppConfig>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = AppConfig::load().expect("Failed to load configuration");

    init_tracing(&config.service_name, &config.environment);
    init_metrics().expect("Failed to initialize metrics");

    let db_pool = create_pool(&config.database).await?;
    let redis_pool = create_redis_pool(&config.redis)?;
    let cache = Arc::new(CacheClient::new(redis_pool));

    let jwt_service = Arc::new(JwtService::new(
        &config.auth.jwt_secret,
        config.auth.jwt_expiry_secs,
    ));

    let kafka = Arc::new(KafkaProducer::new(&config.kafka)?);

    let message_repo = Arc::new(MessageRepository::new(db_pool));
    let connections: ConnectionRegistry = Arc::new(DashMap::new());

    let chat_service = Arc::new(ChatService::new(
        Arc::clone(&message_repo),
        Arc::clone(&cache),
        Arc::clone(&connections),
        Arc::clone(&kafka),
        config.kafka.chat_messages_topic.clone(),
    ));

    let state = Arc::new(AppState {
        chat_service: Arc::clone(&chat_service),
        connections: Arc::clone(&connections),
        jwt_service: Arc::clone(&jwt_service),
        config: Arc::new(config.clone()),
    });

    // Background: subscribe to Redis presence channel and push presence updates
    // to online followers whose connections are in this instance's registry.
    let presence_sub = PresenceSubscriber::new(
        Arc::clone(&connections),
        config.redis.url.clone(),
    );
    tokio::spawn(async move {
        loop {
            if let Err(e) = presence_sub.run().await {
                tracing::error!(error = %e, "Presence subscriber crashed — restarting in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    let app = api::routes::build_router(state)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        // WebSocket connections are long-lived — timeout only applies to the
        // HTTP upgrade handshake, not the WebSocket session itself.
        .layer(TimeoutLayer::new(Duration::from_secs(
            config.server.request_timeout_secs,
        )));

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!(address = %addr, service = "chat-service", "Server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

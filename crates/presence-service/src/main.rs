use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::info;

use shared::{
    cache::{create_redis_pool, CacheClient},
    config::AppConfig,
    observability::{init_metrics, init_tracing},
};

mod api;
mod domain;
mod service;

use service::presence_service::PresenceService;

pub struct AppState {
    pub presence_service: Arc<PresenceService>,
    pub config: Arc<AppConfig>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = AppConfig::load().expect("Failed to load configuration");

    init_tracing(&config.service_name, &config.environment);
    init_metrics().expect("Failed to initialize metrics");

    let redis_pool = create_redis_pool(&config.redis)?;
    let cache = Arc::new(CacheClient::new(redis_pool));

    let presence_service = Arc::new(PresenceService::new(
        Arc::clone(&cache),
        config.redis.presence_ttl_secs,
    ));

    let state = Arc::new(AppState {
        presence_service,
        config: Arc::new(config.clone()),
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
        .layer(TimeoutLayer::new(Duration::from_secs(
            config.server.request_timeout_secs,
        )));

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!(address = %addr, service = "presence-service", "Server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

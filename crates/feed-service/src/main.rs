use std::sync::Arc;
use std::time::Duration;

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
    db::create_pool,
    observability::{init_metrics, init_tracing},
};

mod api;
mod domain;
mod repository;
mod service;
mod workers;

use repository::feed_repo::FeedRepository;
use service::{fanout_service::FanoutService, feed_service::FeedService};
use workers::fanout_worker::FanoutWorker;

pub struct AppState {
    pub feed_service: Arc<FeedService>,
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

    let feed_repo = Arc::new(FeedRepository::new(db_pool));
    let fanout_service = Arc::new(FanoutService::new(
        Arc::clone(&feed_repo),
        Arc::clone(&cache),
        config.feed.clone(),
    ));
    let feed_service = Arc::new(FeedService::new(
        Arc::clone(&fanout_service),
        config.feed.feed_page_size,
    ));

    let state = Arc::new(AppState {
        feed_service: Arc::clone(&feed_service),
        config: Arc::new(config.clone()),
    });

    // Spawn Kafka fanout worker in background
    let worker = FanoutWorker::new(
        Arc::clone(&fanout_service),
        config.kafka.brokers.clone(),
        config.kafka.post_events_topic.clone(),
        config.kafka.consumer_group_id.clone(),
    );

    tokio::spawn(async move {
        loop {
            if let Err(e) = worker.run().await {
                tracing::error!(error = %e, "Fanout worker crashed — restarting in 5s");
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
        .layer(TimeoutLayer::new(Duration::from_secs(
            config.server.request_timeout_secs,
        )));

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!(address = %addr, service = "feed-service", "Server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

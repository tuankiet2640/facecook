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
    kafka::KafkaProducer,
    observability::{init_metrics, init_tracing},
};

mod api;
mod domain;
mod repository;
mod service;

use repository::post_repo::PostRepository;
use service::post_service::PostService;

pub struct AppState {
    pub post_service: Arc<PostService>,
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
    let kafka = Arc::new(KafkaProducer::new(&config.kafka)?);

    let post_repo = Arc::new(PostRepository::new(db_pool));
    let post_service = Arc::new(PostService::new(
        post_repo,
        cache,
        kafka,
        config.kafka.post_events_topic.clone(),
        config.kafka.feed_fanout_topic.clone(),
    ));

    let state = Arc::new(AppState {
        post_service,
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
    info!(address = %addr, service = "post-service", "Server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

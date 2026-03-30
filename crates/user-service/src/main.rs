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
    auth::JwtService,
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

use repository::user_repo::UserRepository;
use service::user_service::UserService;

pub struct AppState {
    pub user_service: Arc<UserService>,
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
    let kafka = Arc::new(KafkaProducer::new(&config.kafka)?);
    let jwt_service = Arc::new(JwtService::new(
        &config.auth.jwt_secret,
        config.auth.jwt_expiry_secs,
    ));

    let user_repo = Arc::new(UserRepository::new(db_pool.clone()));
    let user_service = Arc::new(UserService::new(
        user_repo,
        Arc::clone(&cache),
        Arc::clone(&kafka),
        config.kafka.notification_topic.clone(),
    ));

    let state = Arc::new(AppState {
        user_service,
        jwt_service,
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
    info!(address = %addr, service = "user-service", "Server listening");

    axum::serve(listener, app).await?;
    Ok(())
}

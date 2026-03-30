use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

use shared::{
    auth::JwtService,
    cache::{create_redis_pool, CacheClient},
    config::AppConfig,
    observability::{init_metrics, init_tracing},
};

mod middleware;
mod router;

pub struct GatewayState {
    pub jwt_service: Arc<JwtService>,
    pub cache: Arc<CacheClient>,
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
    let jwt_service = Arc::new(JwtService::new(
        &config.auth.jwt_secret,
        config.auth.jwt_expiry_secs,
    ));

    let state = Arc::new(GatewayState {
        jwt_service,
        cache,
        config: Arc::new(config.clone()),
    });

    let app = router::build_router(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;

    info!(address = %addr, service = "gateway", "Server listening");

    axum::serve(listener, app).await?;

    Ok(())
}

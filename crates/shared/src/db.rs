use sqlx::{
    postgres::{PgPool, PgPoolOptions},
    ConnectOptions,
};
use std::time::Duration;
use tracing::log::LevelFilter;

use crate::config::DatabaseConfig;
use crate::errors::AppError;

pub type DbPool = PgPool;

/// Create a PostgreSQL connection pool with tuned parameters.
///
/// - max_connections: set based on service load (feed service needs more)
/// - min_connections: keep warm connections to avoid cold-start latency
/// - connect_timeout: fail fast if DB is unreachable
/// - acquire_timeout: return 503 rather than waiting forever under load
pub async fn create_pool(config: &DatabaseConfig) -> Result<DbPool, AppError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .connect_with(
            config
                .url
                .parse::<sqlx::postgres::PgConnectOptions>()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid database URL: {}", e)))?
                .log_statements(LevelFilter::Debug)
                .log_slow_statements(LevelFilter::Warn, Duration::from_secs(1)),
        )
        .await
        .map_err(|e| AppError::Database(e))?;

    tracing::info!(
        max_connections = config.max_connections,
        min_connections = config.min_connections,
        "Database pool created"
    );

    Ok(pool)
}

/// Run pending migrations. Called at service startup.
pub async fn run_migrations(pool: &DbPool) -> Result<(), AppError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Migration failed: {}", e)))?;
    tracing::info!("Database migrations applied");
    Ok(())
}

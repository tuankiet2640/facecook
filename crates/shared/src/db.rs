use sqlx::{
    postgres::{PgPool, PgPoolOptions},
    ConnectOptions, Executor,
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
        // PgBouncer transaction-mode pooling (Supabase pooler port 6543) shares
        // backend Postgres connections across transactions. sqlx names its
        // prepared statements with a per-Connection counter (sqlx_s_0, sqlx_s_1,
        // ...) starting from zero, so when two sqlx Connections happen to be
        // assigned the same backend by PgBouncer, their counters collide and we
        // get "prepared statement \"sqlx_s_N\" already exists" errors.
        //
        // statement_cache_capacity(0) alone is NOT enough — it only drops
        // sqlx's local statement handle; the statement is still allocated on
        // the backend until the session ends. Running DEALLOCATE ALL every
        // time the pool hands us a backend wipes any orphaned statements left
        // by the previous tenant before our sqlx Connection starts numbering.
        .before_acquire(|conn, _meta| {
            Box::pin(async move {
                conn.execute("DEALLOCATE ALL").await?;
                Ok(true)
            })
        })
        .connect_with(
            config
                .url
                .parse::<sqlx::postgres::PgConnectOptions>()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("Invalid database URL: {}", e)))?
                .log_statements(LevelFilter::Debug)
                .log_slow_statements(LevelFilter::Warn, Duration::from_secs(1))
                .statement_cache_capacity(0),
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

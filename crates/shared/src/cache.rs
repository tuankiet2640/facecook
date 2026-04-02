use deadpool_redis::{Config as RedisPoolConfig, Pool, Runtime};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::config::RedisConfig;
use crate::errors::AppError;

pub type RedisPool = Pool;

pub fn create_redis_pool(config: &RedisConfig) -> Result<RedisPool, AppError> {
    let cfg = RedisPoolConfig::from_url(&config.url);
    cfg.create_pool(Some(Runtime::Tokio1))
        .map_err(|e| AppError::Cache(e.to_string()))
}

/// Wrapper around deadpool-redis providing typed, ergonomic access to Redis.
/// All serialization is done with serde_json for observability and debuggability.
pub struct CacheClient {
    pool: RedisPool,
}

impl CacheClient {
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    pub async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> Result<Option<T>, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let val: Option<String> = conn
            .get(key)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        match val {
            Some(s) => Ok(Some(
                serde_json::from_str(&s).map_err(|e| AppError::Cache(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl_secs: u64) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let serialized =
            serde_json::to_string(value).map_err(|e| AppError::Cache(e.to_string()))?;
        conn.set_ex(key, serialized, ttl_secs)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    pub async fn del(&self, key: &str) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.del(key)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    /// Atomic set-if-not-exists with TTL — used for idempotency keys.
    /// Returns true if key was set (first occurrence), false if already existed.
    pub async fn set_nx(&self, key: &str, value: &str, ttl_secs: u64) -> Result<bool, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let result: bool = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        Ok(result)
    }

    /// Increment an integer counter. Creates key at 0 before incrementing if missing.
    pub async fn incr(&self, key: &str) -> Result<i64, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.incr(key, 1i64)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    /// Add member to sorted set with given score.
    pub async fn zadd(&self, key: &str, score: f64, member: &str) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.zadd(key, member, score)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    /// Get sorted set range in descending order with scores.
    pub async fn zrevrange_with_scores(
        &self,
        key: &str,
        start: isize,
        stop: isize,
    ) -> Result<Vec<(String, f64)>, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.zrevrange_withscores(key, start, stop)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    /// Remove elements by rank range. Used to trim feed to max size.
    /// zremrangebyrank(key, 0, -(max+1)) removes oldest entries.
    pub async fn zremrangebyrank(
        &self,
        key: &str,
        start: isize,
        stop: isize,
    ) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.zremrangebyrank(key, start, stop)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    /// Sliding window rate limiter using sorted sets.
    /// Returns true if request is within limit, false if rate limited.
    pub async fn check_rate_limit(
        &self,
        key: &str,
        limit: u64,
        window_secs: u64,
    ) -> Result<bool, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;

        let now = chrono::Utc::now().timestamp_millis() as f64;
        let window_start = now - (window_secs as f64 * 1000.0);
        let now_str = now.to_string();

        // Atomic pipeline: remove stale, add current, count, set TTL
        let (count,): (u64,) = redis::pipe()
            .atomic()
            .cmd("ZREMRANGEBYSCORE")
            .arg(key)
            .arg("-inf")
            .arg(window_start)
            .cmd("ZADD")
            .arg(key)
            .arg(now)
            .arg(&now_str)
            .cmd("ZCARD")
            .arg(key)
            .cmd("EXPIRE")
            .arg(key)
            .arg(window_secs)
            .ignore()
            .ignore()
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;

        Ok(count <= limit)
    }

    /// Publish message to Redis pub/sub channel.
    pub async fn publish(&self, channel: &str, message: &str) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.publish(channel, message)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    pub async fn hset<T: Serialize>(
        &self,
        hash: &str,
        field: &str,
        value: &T,
    ) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let serialized =
            serde_json::to_string(value).map_err(|e| AppError::Cache(e.to_string()))?;
        conn.hset(hash, field, serialized)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    pub async fn hget<T: for<'de> Deserialize<'de>>(
        &self,
        hash: &str,
        field: &str,
    ) -> Result<Option<T>, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let val: Option<String> = conn
            .hget(hash, field)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        match val {
            Some(s) => Ok(Some(
                serde_json::from_str(&s).map_err(|e| AppError::Cache(e.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    pub async fn set_with_ttl(
        &self,
        key: &str,
        value: &str,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        conn.set_ex(key, value, ttl_secs)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))
    }

    pub async fn exists(&self, key: &str) -> Result<bool, AppError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        let count: u64 = conn
            .exists(key)
            .await
            .map_err(|e| AppError::Cache(e.to_string()))?;
        Ok(count > 0)
    }

    pub fn pool(&self) -> &RedisPool {
        &self.pool
    }
}

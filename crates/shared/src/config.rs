use config::{Config, ConfigError, Environment};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub acquire_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: usize,
    pub feed_ttl_secs: u64,
    pub presence_ttl_secs: u64,
    pub session_ttl_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KafkaConfig {
    pub brokers: String,
    pub consumer_group_id: String,
    pub post_events_topic: String,
    pub feed_fanout_topic: String,
    pub chat_messages_topic: String,
    pub notification_topic: String,
    pub message_timeout_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwt_expiry_secs: u64,
    pub jwt_refresh_expiry_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub request_timeout_secs: u64,
    pub max_request_body_bytes: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FeedConfig {
    /// Followers above this threshold use fanout-on-read (celebrity problem)
    pub celebrity_threshold: i64,
    /// Maximum number of items stored in a user's feed sorted set
    pub max_feed_size: i64,
    pub feed_page_size: i32,
    pub post_cache_ttl_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub service_name: String,
    pub environment: String,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub kafka: KafkaConfig,
    pub auth: AuthConfig,
    pub feed: FeedConfig,
}

impl AppConfig {
    /// Load configuration from environment variables using double-underscore as separator.
    /// e.g. DATABASE__URL maps to database.url
    pub fn load() -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(Environment::default().separator("__").try_parsing(true))
            .build()?
            .try_deserialize()
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production"
    }
}

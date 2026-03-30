use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Post {
    pub id: Uuid,
    pub author_id: Uuid,
    pub content: String,
    pub media_urls: serde_json::Value, // JSONB array of URLs
    pub like_count: i64,
    pub comment_count: i64,
    pub share_count: i64,
    pub visibility: PostVisibility,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "post_visibility", rename_all = "lowercase")]
pub enum PostVisibility {
    Public,
    Friends,
    Private,
}

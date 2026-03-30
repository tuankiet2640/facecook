use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Hydrated feed response: post IDs resolved to full post objects.
#[derive(Debug, Serialize, Deserialize)]
pub struct FeedResponse {
    pub items: Vec<FeedItem>,
    /// Cursor for next page (score of last item). Client passes as `before_score` param.
    pub next_cursor: Option<f64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub post_id: Uuid,
    /// Unix epoch milliseconds as f64 — used as Redis sorted set score.
    pub score: f64,
}

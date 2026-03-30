use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single item in a user's feed, referencing a post by ID with a score
/// (unix epoch milliseconds) used for chronological ordering in Redis sorted sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedEntry {
    pub post_id: Uuid,
    pub author_id: Uuid,
    /// Score = timestamp as f64 (unix epoch ms). Higher = newer.
    pub score: f64,
}

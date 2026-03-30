use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Published to Kafka when a user registers.
/// Consumed by notification service to send welcome email.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegistered {
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
}

/// Published when a user follows another.
/// Consumed by feed-service to potentially pre-warm follow's feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFollowed {
    pub follower_id: Uuid,
    pub followee_id: Uuid,
}

/// Published when a user unfollows another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUnfollowed {
    pub follower_id: Uuid,
    pub followee_id: Uuid,
}

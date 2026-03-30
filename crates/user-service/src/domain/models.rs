use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(length(min = 3, max = 50, message = "Username must be 3–50 characters"))]
    #[validate(regex(
        path = "USERNAME_RE",
        message = "Username may only contain letters, digits, underscores, and hyphens"
    ))]
    pub username: String,

    #[validate(email(message = "Invalid email address"))]
    pub email: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,

    #[validate(length(min = 1, max = 100, message = "Display name must be 1–100 characters"))]
    pub display_name: String,
}

lazy_static::lazy_static! {
    static ref USERNAME_RE: regex::Regex = regex::Regex::new(r"^[a-zA-Z0-9_\-]+$").unwrap();
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub user_id: Uuid,
    pub username: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProfileRequest {
    #[validate(length(min = 1, max = 100))]
    pub display_name: Option<String>,

    #[validate(length(max = 500))]
    pub bio: Option<String>,

    #[validate(url)]
    pub avatar_url: Option<String>,
}

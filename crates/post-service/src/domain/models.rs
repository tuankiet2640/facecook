use serde::Deserialize;
use validator::Validate;

use shared::models::post::PostVisibility;

#[derive(Debug, Deserialize, Validate)]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 5000, message = "Content must be 1–5000 characters"))]
    pub content: String,
    #[validate(length(max = 10, message = "Maximum 10 media attachments"))]
    pub media_urls: Vec<String>,
    pub visibility: Option<PostVisibility>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1, max = 5000))]
    pub content: Option<String>,
    pub visibility: Option<PostVisibility>,
}

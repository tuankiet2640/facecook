use serde::Deserialize;
use validator::Validate;

use shared::models::post::PostVisibility;

#[derive(Debug, Deserialize, Validate)]
pub struct CreatePostRequest {
    // Content may be empty as long as at least one media attachment is present.
    // The caller-level check (handler) enforces "content OR media must exist".
    #[serde(default)]
    #[validate(length(max = 5000, message = "Content must be at most 5000 characters"))]
    pub content: String,
    #[serde(default)]
    #[validate(length(max = 10, message = "Maximum 10 media attachments"))]
    pub media_urls: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub visibility: Option<PostVisibility>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1, max = 5000))]
    pub content: Option<String>,
    pub visibility: Option<PostVisibility>,
}

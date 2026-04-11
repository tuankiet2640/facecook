use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use shared::{
    errors::{AppError, AppResult},
    observability::health_check,
};

use crate::{domain::models::CreatePostRequest, AppState};

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/posts", post(create_post))
        .route("/api/v1/posts/batch", get(get_posts_batch))
        .route("/api/v1/posts/:post_id", get(get_post).delete(delete_post))
        .route("/api/v1/users/:user_id/posts", get(get_user_posts))
        .with_state(state)
}

async fn create_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreatePostRequest>,
) -> AppResult<Json<serde_json::Value>> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    // A post must carry *something* — either text or media. Empty-everything
    // posts are rejected here rather than in the validator derive because this
    // is a cross-field rule.
    if req.content.trim().is_empty() && req.media_urls.is_empty() {
        return Err(AppError::Validation(
            "Post must have content or at least one media attachment".to_string(),
        ));
    }
    let author_id = extract_user_id(&headers)?;
    let post = state.post_service.create_post(author_id, req).await?;
    Ok(Json(serde_json::to_value(&post).unwrap()))
}

async fn get_post(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let post = state.post_service.get_post(post_id).await?;
    Ok(Json(serde_json::to_value(&post).unwrap()))
}

async fn delete_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    state.post_service.delete_post(post_id, user_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct BatchQuery {
    ids: String, // comma-separated UUIDs
}

async fn get_posts_batch(
    State(state): State<Arc<AppState>>,
    Query(q): Query<BatchQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let post_ids: Vec<Uuid> = q.ids
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .collect();

    if post_ids.len() > 100 {
        return Err(AppError::BadRequest(
            "Maximum 100 post IDs per batch request".to_string(),
        ));
    }
    let posts = state.post_service.get_posts_by_ids(post_ids).await?;
    Ok(Json(serde_json::json!({ "data": posts })))
}

#[derive(Deserialize)]
struct UserPostsQuery {
    limit: Option<i64>,
    before: Option<DateTime<Utc>>,
}

async fn get_user_posts(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(q): Query<UserPostsQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(20).min(50);
    let posts = state
        .post_service
        .get_user_posts(user_id, limit, q.before)
        .await?;
    Ok(Json(serde_json::json!({ "data": posts })))
}

fn extract_user_id(headers: &HeaderMap) -> AppResult<Uuid> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid X-User-Id header".to_string()))
}

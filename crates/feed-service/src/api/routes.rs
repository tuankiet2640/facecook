use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use shared::{
    errors::{AppError, AppResult},
    observability::health_check,
};

use crate::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/feed", get(get_feed))
        .with_state(state)
}

#[derive(Deserialize)]
struct FeedQuery {
    limit: Option<i32>,
    /// Cursor from previous page (score of last item — unix epoch ms as f64)
    before_score: Option<f64>,
}

async fn get_feed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<FeedQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    let feed = state
        .feed_service
        .get_feed(user_id, q.before_score, q.limit)
        .await?;
    Ok(Json(serde_json::to_value(&feed).unwrap()))
}

fn extract_user_id(headers: &HeaderMap) -> AppResult<Uuid> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid X-User-Id header".to_string()))
}

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use shared::{
    errors::{AppError, AppResult},
    observability::health_check,
};

use crate::{
    domain::models::{LoginRequest, RegisterRequest, UpdateProfileRequest},
    AppState,
};

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/users/me", get(get_me).put(update_profile))
        .route("/api/v1/users/:user_id", get(get_user))
        .route("/api/v1/users/username/:username", get(get_user_by_username))
        .route("/api/v1/users/:user_id/follow", post(follow_user))
        .route("/api/v1/users/:user_id/unfollow", delete(unfollow_user))
        .route("/api/v1/users/:user_id/followers", get(get_followers))
        .route("/api/v1/users/:user_id/following", get(get_following))
        .with_state(state)
}

async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<serde_json::Value>> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let (user, _) = state.user_service.register(req).await?;
    let token = state
        .jwt_service
        .issue_token(user.id, &user.username, &user.email)?;

    Ok(Json(serde_json::json!({
        "access_token": token,
        "token_type": "Bearer",
        "expires_in": state.config.auth.jwt_expiry_secs,
        "user_id": user.id,
        "username": user.username,
    })))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<serde_json::Value>> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let resp = state
        .user_service
        .login(req, &state.jwt_service)
        .await?;

    Ok(Json(serde_json::json!({
        "access_token": resp.access_token,
        "token_type": resp.token_type,
        "expires_in": resp.expires_in,
        "user_id": resp.user_id,
        "username": resp.username,
    })))
}

async fn get_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    let profile = state.user_service.get_profile(user_id).await?;
    Ok(Json(serde_json::to_value(profile).unwrap()))
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> AppResult<Json<serde_json::Value>> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let user_id = extract_user_id(&headers)?;
    let profile = state.user_service.update_profile(user_id, req).await?;
    Ok(Json(serde_json::to_value(profile).unwrap()))
}

async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let profile = state.user_service.get_profile(user_id).await?;
    Ok(Json(serde_json::to_value(profile).unwrap()))
}

async fn get_user_by_username(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let profile = state
        .user_service
        .get_profile_by_username(&username)
        .await?;
    Ok(Json(serde_json::to_value(profile).unwrap()))
}

async fn follow_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(followee_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let follower_id = extract_user_id(&headers)?;
    state.user_service.follow(follower_id, followee_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn unfollow_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(followee_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let follower_id = extract_user_id(&headers)?;
    state
        .user_service
        .unfollow(follower_id, followee_id)
        .await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct PaginationQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn get_followers(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let limit = pagination.limit.unwrap_or(20).min(100);
    let offset = pagination.offset.unwrap_or(0);
    let followers = state
        .user_service
        .get_followers(user_id, limit, offset)
        .await?;
    Ok(Json(serde_json::json!({ "data": followers, "limit": limit, "offset": offset })))
}

async fn get_following(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Query(pagination): Query<PaginationQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let limit = pagination.limit.unwrap_or(20).min(100);
    let offset = pagination.offset.unwrap_or(0);
    let following = state
        .user_service
        .get_following(user_id, limit, offset)
        .await?;
    Ok(Json(serde_json::json!({ "data": following, "limit": limit, "offset": offset })))
}

/// Extract authenticated user ID from X-User-Id header (set by gateway).
fn extract_user_id(headers: &HeaderMap) -> AppResult<Uuid> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid X-User-Id header".to_string()))
}

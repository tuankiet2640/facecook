use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use uuid::Uuid;

use shared::{
    errors::{AppError, AppResult},
    observability::health_check,
};

use crate::{domain::models::BatchPresenceRequest, AppState};

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/presence/:user_id", get(get_presence))
        .route("/api/v1/presence/batch", post(get_batch))
        // These are called by the chat service (service-to-service), not public users.
        // The gateway should not expose /internal routes externally.
        .route("/internal/presence/online", post(set_online))
        .route("/internal/presence/offline", post(set_offline))
        .route("/internal/presence/heartbeat", post(heartbeat))
        .with_state(state)
}

async fn get_presence(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let status = state
        .presence_service
        .get_presence(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No presence record for {}", user_id)))?;
    Ok(Json(serde_json::to_value(&status).unwrap()))
}

async fn get_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchPresenceRequest>,
) -> AppResult<Json<serde_json::Value>> {
    if req.user_ids.len() > 200 {
        return Err(AppError::BadRequest(
            "Batch size exceeds limit of 200".to_string(),
        ));
    }
    let statuses = state.presence_service.get_batch(req.user_ids).await?;
    Ok(Json(serde_json::json!({ "data": statuses })))
}

// ── Internal endpoints (service-to-service) ───────────────────────────────────

#[derive(serde::Deserialize)]
struct UserIdBody {
    user_id: Uuid,
}

async fn set_online(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UserIdBody>,
) -> AppResult<Json<serde_json::Value>> {
    state.presence_service.set_online(body.user_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn set_offline(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UserIdBody>,
) -> AppResult<Json<serde_json::Value>> {
    state.presence_service.set_offline(body.user_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn heartbeat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    state.presence_service.heartbeat(user_id).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

fn extract_user_id(headers: &HeaderMap) -> AppResult<Uuid> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid X-User-Id header".to_string()))
}

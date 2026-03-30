use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use metrics::counter;

use shared::{
    auth::extract_bearer_token,
    errors::AppError,
};

use crate::GatewayState;

/// JWT authentication middleware.
///
/// Extracts the Authorization: Bearer <token> header, validates the JWT,
/// and injects X-User-Id / X-Username headers for downstream services.
/// Downstream services trust these headers since they're internal to the cluster.
///
/// Public routes (auth/register, auth/login) are exempted in the router.
pub async fn auth_middleware(
    State(state): State<Arc<GatewayState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    // Allow public paths to bypass auth
    let path = req.uri().path();
    if is_public_path(path) {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

    let token = extract_bearer_token(auth_header)
        .ok_or_else(|| AppError::Unauthorized("Invalid Authorization header format".to_string()))?;

    let claims = state.jwt_service.validate_token(token)?;

    // Inject user context headers for downstream services
    let headers = req.headers_mut();
    headers.insert(
        "x-user-id",
        claims.sub.to_string().parse().map_err(|_| {
            AppError::Internal(anyhow::anyhow!("Failed to parse user_id as header value"))
        })?,
    );
    headers.insert(
        "x-username",
        claims.username.parse().map_err(|_| {
            AppError::Internal(anyhow::anyhow!("Failed to parse username as header value"))
        })?,
    );

    counter!("http_requests_total", "authenticated" => "true").increment(1);

    Ok(next.run(req).await)
}

fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/health" | "/api/v1/auth/register" | "/api/v1/auth/login"
    )
}

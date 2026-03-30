use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::Request,
    middleware as axum_middleware,
    response::Response,
    routing::{any, get},
    Router,
};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use shared::observability::health_check;

use crate::{
    middleware::{auth::auth_middleware, rate_limit::RateLimitLayer},
    GatewayState,
};

pub fn build_router(state: Arc<GatewayState>) -> Router {
    let timeout_secs = state.config.server.request_timeout_secs;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(86400));

    Router::new()
        .route("/health", get(health_check))
        // All API routes go through auth middleware
        .nest("/api/v1", api_routes(state.clone()))
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(timeout_secs)))
        .with_state(state)
}

fn api_routes(state: Arc<GatewayState>) -> Router<Arc<GatewayState>> {
    Router::new()
        // Public routes (no auth required)
        .route("/auth/register", any(proxy_to_user_service))
        .route("/auth/login", any(proxy_to_user_service))
        // Protected routes
        .route("/users/*path", any(proxy_to_user_service))
        .route("/posts/*path", any(proxy_to_post_service))
        .route("/feed/*path", any(proxy_to_feed_service))
        .route("/chat/*path", any(proxy_to_chat_service))
        .route("/presence/*path", any(proxy_to_presence_service))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
}

/// Proxy handlers — in production these would use reqwest to forward requests.
/// The gateway pattern: validate JWT, strip/add headers, forward downstream.
async fn proxy_to_user_service(req: Request) -> Response {
    proxy_request(req, "http://user-service:8081").await
}

async fn proxy_to_post_service(req: Request) -> Response {
    proxy_request(req, "http://post-service:8083").await
}

async fn proxy_to_feed_service(req: Request) -> Response {
    proxy_request(req, "http://feed-service:8082").await
}

async fn proxy_to_chat_service(req: Request) -> Response {
    proxy_request(req, "http://chat-service:8084").await
}

async fn proxy_to_presence_service(req: Request) -> Response {
    proxy_request(req, "http://presence-service:8085").await
}

async fn proxy_request(req: Request, _upstream: &str) -> Response {
    // In a real deployment, use reqwest or hyper to forward the request.
    // The gateway validates JWT and then forwards with X-User-Id header set.
    axum::response::Response::builder()
        .status(200)
        .body(axum::body::Body::empty())
        .unwrap()
}

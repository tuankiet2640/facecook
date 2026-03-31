use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
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

/// Upstream service base URLs.
/// In production these come from service discovery / env vars.
const USER_SERVICE_URL: &str = "http://user-service:8081";
const POST_SERVICE_URL: &str = "http://post-service:8083";
const FEED_SERVICE_URL: &str = "http://feed-service:8082";
const CHAT_SERVICE_URL: &str = "http://chat-service:8084";
const PRESENCE_SERVICE_URL: &str = "http://presence-service:8085";

pub fn build_router(state: Arc<GatewayState>) -> Router {
    let timeout_secs = state.config.server.request_timeout_secs;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(Duration::from_secs(86400));

    Router::new()
        .route("/health", get(health_check))
        .nest("/api/v1", api_routes(state.clone()))
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(timeout_secs)))
        .with_state(state)
}

fn api_routes(state: Arc<GatewayState>) -> Router<Arc<GatewayState>> {
    Router::new()
        // Public — auth middleware exempts these paths
        .route("/auth/register", any(proxy_user_service))
        .route("/auth/login", any(proxy_user_service))
        // Protected
        .route("/users/*path", any(proxy_user_service))
        .route("/posts/*path", any(proxy_post_service))
        .route("/feed/*path", any(proxy_feed_service))
        // Chat WebSocket: clients connect directly to chat-service on its port.
        // The gateway proxies REST endpoints only; WS upgrade is not proxied
        // because it requires a persistent tunnel between gateway and upstream.
        .route("/chat/*path", any(proxy_chat_service))
        .route("/presence/*path", any(proxy_presence_service))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
}

// ── Per-service proxy handlers ─────────────────────────────────────────────────

async fn proxy_user_service(
    State(state): State<Arc<GatewayState>>,
    req: Request,
) -> Response {
    proxy_request(&state.http_client, req, USER_SERVICE_URL).await
}

async fn proxy_post_service(
    State(state): State<Arc<GatewayState>>,
    req: Request,
) -> Response {
    proxy_request(&state.http_client, req, POST_SERVICE_URL).await
}

async fn proxy_feed_service(
    State(state): State<Arc<GatewayState>>,
    req: Request,
) -> Response {
    proxy_request(&state.http_client, req, FEED_SERVICE_URL).await
}

async fn proxy_chat_service(
    State(state): State<Arc<GatewayState>>,
    req: Request,
) -> Response {
    proxy_request(&state.http_client, req, CHAT_SERVICE_URL).await
}

async fn proxy_presence_service(
    State(state): State<Arc<GatewayState>>,
    req: Request,
) -> Response {
    proxy_request(&state.http_client, req, PRESENCE_SERVICE_URL).await
}

// ── Core proxy logic ───────────────────────────────────────────────────────────

/// Forward an axum `Request` to `upstream`, preserving method, path, query,
/// headers (minus hop-by-hop), and body. Returns the upstream response verbatim.
///
/// The auth middleware has already:
///   1. Validated the JWT
///   2. Injected `X-User-Id` and `X-Username` headers
///
/// This function strips the `Authorization` header before forwarding — downstream
/// services must not re-validate the JWT (they trust the gateway's injected headers).
async fn proxy_request(client: &reqwest::Client, req: Request, upstream: &str) -> Response {
    // Reconstruct the upstream URL preserving path and query string.
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let url = format!("{}{}", upstream, path_and_query);

    let method = req.method().clone();
    let headers = req.headers().clone();

    // Buffer the request body. For large uploads this would stream instead.
    let body_bytes = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "Failed to read request body");
            return error_response(StatusCode::BAD_REQUEST, "BODY_READ_ERROR", "Failed to read request body");
        }
    };

    // Build the forwarded request.
    let mut builder = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        &url,
    );

    // Forward headers, stripping hop-by-hop and Authorization.
    for (name, value) in headers.iter() {
        if should_forward_header(name) {
            if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                builder = builder.header(name.as_str(), v);
            }
        }
    }

    builder = builder.body(body_bytes.to_vec());

    match builder.send().await {
        Ok(upstream_resp) => {
            let status = StatusCode::from_u16(upstream_resp.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

            let mut response_builder = Response::builder().status(status);

            // Forward response headers (minus hop-by-hop).
            for (name, value) in upstream_resp.headers().iter() {
                if should_forward_header(name) {
                    if let (Ok(n), Ok(v)) = (
                        HeaderName::from_bytes(name.as_str().as_bytes()),
                        HeaderValue::from_bytes(value.as_bytes()),
                    ) {
                        response_builder = response_builder.header(n, v);
                    }
                }
            }

            let body = upstream_resp.bytes().await.unwrap_or_default();
            response_builder
                .body(Body::from(body))
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "PROXY_ERROR", "Failed to build response"))
        }
        Err(e) => {
            tracing::error!(error = %e, upstream = upstream, "Upstream request failed");
            if e.is_timeout() {
                error_response(StatusCode::GATEWAY_TIMEOUT, "GATEWAY_TIMEOUT", "Upstream service timed out")
            } else {
                error_response(StatusCode::BAD_GATEWAY, "BAD_GATEWAY", "Upstream service unavailable")
            }
        }
    }
}

/// Returns true if the header should be forwarded to/from upstream.
/// Strips hop-by-hop headers per HTTP/1.1 spec (RFC 7230 §6.1).
fn should_forward_header(name: &reqwest::header::HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "authorization" // stripped — replaced by X-User-Id downstream
    )
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": { "code": code, "message": message }
    })
    .to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

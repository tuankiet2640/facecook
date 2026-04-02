#![allow(dead_code)]

use std::{
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    extract::Request,
    response::{IntoResponse, Response},
};
use futures::future::BoxFuture;
use metrics::counter;
use tower::{Layer, Service};

use shared::{cache::CacheClient, errors::AppError};

/// Tier-based rate limits. Each endpoint tier has different limits.
/// All limits use a sliding window algorithm backed by Redis sorted sets.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Default: 100 requests per 60 seconds
    pub default_limit: u64,
    pub default_window_secs: u64,
    /// Auth endpoints: stricter to prevent brute-force
    pub auth_limit: u64,
    pub auth_window_secs: u64,
    /// Write endpoints (post, message): 30 per minute
    pub write_limit: u64,
    pub write_window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            default_limit: 100,
            default_window_secs: 60,
            auth_limit: 10,
            auth_window_secs: 60,
            write_limit: 30,
            write_window_secs: 60,
        }
    }
}

#[derive(Clone)]
pub struct RateLimitLayer {
    cache: Arc<CacheClient>,
    config: RateLimitConfig,
}

impl RateLimitLayer {
    pub fn new(cache: Arc<CacheClient>, config: RateLimitConfig) -> Self {
        Self { cache, config }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            cache: Arc::clone(&self.cache),
            config: self.config.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    cache: Arc<CacheClient>,
    config: RateLimitConfig,
}

impl<S> Service<Request<Body>> for RateLimitMiddleware<S>
where
    S: Service<Request<Body>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let cache = Arc::clone(&self.cache);
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract IP for rate limit key. In production, read X-Forwarded-For.
            let ip = req
                .headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let path = req.uri().path().to_string();
            let (limit, window) = classify_endpoint(&path, &config);
            let rate_key = format!("rate_limit:{}:{}", ip, classify_tier(&path));

            match cache.check_rate_limit(&rate_key, limit, window).await {
                Ok(allowed) if !allowed => {
                    counter!("rate_limit_rejections_total", "path" => path).increment(1);
                    return Ok(AppError::RateLimited.into_response());
                }
                Err(e) => {
                    // Fail open: if Redis is unavailable, allow the request
                    // and log the error. Better UX than cascading failures.
                    tracing::warn!(error = %e, "Rate limit check failed, allowing request");
                }
                _ => {}
            }

            inner.call(req).await
        })
    }
}

fn classify_endpoint(path: &str, config: &RateLimitConfig) -> (u64, u64) {
    if path.contains("/auth/") {
        (config.auth_limit, config.auth_window_secs)
    } else if path.contains("/posts") || path.contains("/chat/messages") {
        (config.write_limit, config.write_window_secs)
    } else {
        (config.default_limit, config.default_window_secs)
    }
}

fn classify_tier(path: &str) -> &str {
    if path.contains("/auth/") {
        "auth"
    } else if path.contains("/posts") || path.contains("/chat/messages") {
        "write"
    } else {
        "default"
    }
}

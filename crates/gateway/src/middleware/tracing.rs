use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use metrics::{counter, histogram};
use uuid::Uuid;

/// Request tracing middleware.
/// Assigns a unique request_id to every request for distributed tracing correlation.
/// Records request duration and status code in Prometheus.
pub async fn tracing_middleware(mut req: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    // Inject request ID so downstream services can log it
    req.headers_mut().insert(
        "x-request-id",
        request_id.parse().unwrap(),
    );

    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    let response = {
        let _enter = span.enter();
        next.run(req).await
    };

    let status = response.status().as_u16().to_string();
    let elapsed = start.elapsed().as_secs_f64();

    counter!(
        "http_requests_total",
        "method" => method.clone(),
        "status" => status.clone(),
    )
    .increment(1);

    histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path,
        "status" => status,
    )
    .record(elapsed);

    response
}

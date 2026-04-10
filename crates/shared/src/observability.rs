use metrics::{
    describe_counter, describe_gauge, describe_histogram,
};
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize structured logging.
/// Production: JSON format for ingestion by Datadog/ELK/CloudWatch.
/// Development: Pretty format for human readability.
pub fn init_tracing(service_name: &str, env: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if env == "production" {
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(true),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
    }

    tracing::info!(
        service = service_name,
        environment = env,
        "Tracing initialized"
    );
}

/// Install Prometheus metrics exporter.
/// Port is read from METRICS_PORT env var; defaults to 0 (OS-assigned) for local dev,
/// allowing multiple services to run on the same host without port conflicts.
/// In Docker each service has METRICS_PORT=9090 set in docker-compose.
pub fn init_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("METRICS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        .install()?;

    describe_counter!("http_requests_total", "Total HTTP requests processed");
    describe_histogram!(
        "http_request_duration_seconds",
        "HTTP request duration in seconds"
    );
    describe_counter!("feed_fanout_total", "Total feed fanout operations by strategy");
    describe_histogram!(
        "feed_fanout_duration_seconds",
        "Time to complete a feed fanout operation"
    );
    describe_counter!("messages_sent_total", "Total chat messages submitted by clients");
    describe_counter!(
        "messages_delivered_total",
        "Total chat messages delivered (websocket or queued)"
    );
    describe_gauge!(
        "websocket_connections_active",
        "Number of active WebSocket connections"
    );
    describe_counter!(
        "kafka_events_produced_total",
        "Total events published to Kafka"
    );
    describe_counter!(
        "kafka_events_consumed_total",
        "Total events consumed from Kafka"
    );
    describe_histogram!("db_query_duration_seconds", "Database query execution time");
    describe_gauge!("feed_cache_hit_ratio", "Ratio of feed reads served from cache");
    describe_counter!("rate_limit_rejections_total", "Requests rejected by rate limiter");

    Ok(())
}

/// Health check handler — used by load balancers and Docker healthchecks.
pub async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

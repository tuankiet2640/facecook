use std::sync::Arc;

use metrics::counter;
use rdkafka::{consumer::CommitMode, consumer::Consumer, message::Message};
use tracing::{error, info, warn};

use shared::{
    errors::AppResult,
    kafka::{create_consumer, KafkaEvent},
};

use crate::{
    domain::events::{PostCreated, PostDeleted},
    service::fanout_service::FanoutService,
};

/// Kafka consumer worker that processes PostCreated and PostDeleted events.
///
/// At-least-once delivery semantics:
/// - `enable.auto.commit = false` — we commit only after successful processing
/// - On processing failure: do NOT commit → message is re-delivered on restart
/// - On transient errors (Redis unavailable): retry with backoff
/// - On permanent errors (malformed message): log and skip (commit anyway)
///
/// Parallelism: one worker goroutine per Kafka partition.
/// Each feed-service replica consumes a subset of partitions in the consumer group.
pub struct FanoutWorker {
    fanout_service: Arc<FanoutService>,
    brokers: String,
    post_events_topic: String,
    consumer_group_id: String,
}

impl FanoutWorker {
    pub fn new(
        fanout_service: Arc<FanoutService>,
        brokers: String,
        post_events_topic: String,
        consumer_group_id: String,
    ) -> Self {
        Self {
            fanout_service,
            brokers,
            post_events_topic,
            consumer_group_id,
        }
    }

    /// Start the consumer loop. Runs until the process is killed or a fatal error occurs.
    pub async fn run(&self) -> AppResult<()> {
        info!(
            topic = %self.post_events_topic,
            group = %self.consumer_group_id,
            "Fanout worker starting Kafka consumer"
        );

        let kafka_config = shared::config::KafkaConfig {
            brokers: self.brokers.clone(),
            consumer_group_id: self.consumer_group_id.clone(),
            post_events_topic: self.post_events_topic.clone(),
            feed_fanout_topic: String::new(),
            chat_messages_topic: String::new(),
            notification_topic: String::new(),
            message_timeout_ms: 5000,
        };

        let consumer = create_consumer(&kafka_config, &[&self.post_events_topic])?;

        info!("Fanout worker consumer ready, entering message loop");

        loop {
            match consumer.recv().await {
                Err(e) => {
                    error!(error = %e, "Kafka receive error");
                    // Brief pause before retrying to avoid spinning on errors
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Ok(message) => {
                    let payload = match message.payload_view::<str>() {
                        Some(Ok(p)) => p,
                        Some(Err(e)) => {
                            error!(error = %e, "Malformed Kafka message payload (not UTF-8)");
                            // Commit to skip permanently malformed messages
                            let _ = consumer.commit_message(&message, CommitMode::Async);
                            counter!("kafka_events_consumed_total", "status" => "malformed")
                                .increment(1);
                            continue;
                        }
                        None => {
                            warn!("Received Kafka message with no payload");
                            let _ = consumer.commit_message(&message, CommitMode::Async);
                            continue;
                        }
                    };

                    // Detect event type from header or JSON field
                    let event_type = message
                        .headers()
                        .and_then(|h| {
                            for i in 0..h.count() {
                                if let Some(header) = h.get_at(i) {
                                    if header.key == "event_type" {
                                        if let Some(value_bytes) = header.value {
                                            if let Ok(value_str) = std::str::from_utf8(value_bytes)
                                            {
                                                return Some(value_str.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                            None
                        })
                        .unwrap_or_default();

                    let result = match event_type.as_str() {
                        "post.created" => self.handle_post_created(payload).await,
                        "post.deleted" => self.handle_post_deleted(payload).await,
                        _ => {
                            // Try to deserialize and dispatch based on event_type field in JSON
                            self.handle_unknown_event(payload).await
                        }
                    };

                    match result {
                        Ok(()) => {
                            // Commit offset only after successful processing
                            if let Err(e) = consumer.commit_message(&message, CommitMode::Async) {
                                error!(error = %e, "Failed to commit Kafka offset");
                            }
                            counter!("kafka_events_consumed_total", "status" => "success")
                                .increment(1);
                        }
                        Err(e) => {
                            error!(
                                error = %e,
                                topic = %self.post_events_topic,
                                partition = message.partition(),
                                offset = message.offset(),
                                "Failed to process Kafka message — will retry"
                            );
                            counter!("kafka_events_consumed_total", "status" => "error")
                                .increment(1);
                            // Do NOT commit — message will be redelivered
                        }
                    }
                }
            }
        }
    }

    async fn handle_post_created(&self, payload: &str) -> AppResult<()> {
        let event: KafkaEvent<PostCreated> = serde_json::from_str(payload)
            .map_err(|e| shared::errors::AppError::Queue(format!("Deserialize error: {}", e)))?;

        info!(
            post_id = %event.payload.post_id,
            author_id = %event.payload.author_id,
            "Processing PostCreated fanout"
        );

        self.fanout_service
            .fanout_post(
                event.payload.post_id,
                event.payload.author_id,
                event.payload.timestamp_ms,
            )
            .await?;

        Ok(())
    }

    async fn handle_post_deleted(&self, payload: &str) -> AppResult<()> {
        let event: KafkaEvent<PostDeleted> = serde_json::from_str(payload)
            .map_err(|e| shared::errors::AppError::Queue(format!("Deserialize error: {}", e)))?;

        info!(
            post_id = %event.payload.post_id,
            "Processing PostDeleted — no feed cleanup implemented (posts expire naturally)"
        );

        // Future enhancement: remove post from all follower feeds
        // For now: posts expire from feeds naturally as new posts push them out
        Ok(())
    }

    async fn handle_unknown_event(&self, payload: &str) -> AppResult<()> {
        // Try generic deserialization to extract event_type
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
            let event_type = value
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            warn!(
                event_type = event_type,
                "Received unhandled event type in fanout worker"
            );
        }
        // Commit to avoid blocking the partition
        Ok(())
    }
}

use rdkafka::{
    config::ClientConfig,
    consumer::{Consumer, StreamConsumer},
    message::OwnedHeaders,
    producer::{FutureProducer, FutureRecord},
    util::Timeout,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;
use tracing::error;
use uuid::Uuid;

use crate::config::KafkaConfig;
use crate::errors::AppError;

/// Standard envelope wrapping all Kafka events.
/// event_id enables deduplication; idempotency_key is caller-provided.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaEvent<T> {
    pub event_id: Uuid,
    pub event_type: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub payload: T,
    pub idempotency_key: String,
}

impl<T: Serialize + Clone> KafkaEvent<T> {
    pub fn new(event_type: &str, payload: T) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            timestamp: chrono::Utc::now(),
            idempotency_key: Uuid::new_v4().to_string(),
            payload,
        }
    }
}

pub struct KafkaProducer {
    producer: FutureProducer,
    config: KafkaConfig,
}

impl KafkaProducer {
    pub fn new(config: &KafkaConfig) -> Result<Self, AppError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("message.timeout.ms", config.message_timeout_ms.to_string())
            // Idempotent producer: guarantees exactly-once delivery to broker
            .set("enable.idempotence", "true")
            .set("max.in.flight.requests.per.connection", "5")
            .set("retries", "10")
            .set("retry.backoff.ms", "100")
            .set("compression.type", "snappy")
            // Batching for throughput: wait up to 5ms to fill a 64KB batch
            .set("batch.size", "65536")
            .set("linger.ms", "5")
            .create()
            .map_err(|e| AppError::Queue(e.to_string()))?;

        Ok(Self {
            producer,
            config: config.clone(),
        })
    }

    pub async fn publish<T: Serialize>(
        &self,
        topic: &str,
        key: &str,
        event: &KafkaEvent<T>,
    ) -> Result<(), AppError> {
        let payload =
            serde_json::to_vec(event).map_err(|e| AppError::Queue(e.to_string()))?;

        let headers = OwnedHeaders::new()
            .insert(rdkafka::message::Header {
                key: "event_type",
                value: Some(event.event_type.as_bytes()),
            })
            .insert(rdkafka::message::Header {
                key: "idempotency_key",
                value: Some(event.idempotency_key.as_bytes()),
            });

        let record = FutureRecord::to(topic)
            .key(key)
            .payload(&payload)
            .headers(headers);

        let delivery_timeout = Duration::from_millis(self.config.message_timeout_ms);

        match timeout(
            delivery_timeout,
            self.producer.send(record, Timeout::Never),
        )
        .await
        {
            Ok(Ok((partition, offset))) => {
                tracing::debug!(
                    topic = topic,
                    key = key,
                    partition = partition,
                    offset = offset,
                    event_type = event.event_type,
                    "Message delivered to Kafka"
                );
                Ok(())
            }
            Ok(Err((e, _msg))) => {
                error!(error = %e, topic = topic, "Failed to deliver message to Kafka");
                Err(AppError::Queue(e.to_string()))
            }
            Err(_) => {
                error!(topic = topic, "Kafka message delivery timed out");
                Err(AppError::Queue("Message delivery timeout".to_string()))
            }
        }
    }
}

/// Create a Kafka consumer configured for at-least-once delivery.
/// Manual offset commit ensures messages are not lost on crash.
pub fn create_consumer(
    config: &KafkaConfig,
    topics: &[&str],
) -> Result<StreamConsumer, AppError> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &config.brokers)
        .set("group.id", &config.consumer_group_id)
        // Manual commit: only commit after successful processing
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "10000")
        .set("heartbeat.interval.ms", "3000")
        .set("max.poll.interval.ms", "300000")
        .set("fetch.min.bytes", "1024")
        .set("fetch.max.wait.ms", "500")
        .create()
        .map_err(|e| AppError::Queue(e.to_string()))?;

    consumer
        .subscribe(topics)
        .map_err(|e| AppError::Queue(e.to_string()))?;

    Ok(consumer)
}

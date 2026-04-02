use std::sync::Arc;

use futures::StreamExt;
use tracing::{error, info, warn};

use shared::{errors::AppResult, models::message::WsMessage};

use crate::{service::chat_service::PresenceEvent, ConnectionRegistry};

/// Subscribes to the `presence_changes` Redis pub/sub channel.
///
/// When a user's presence changes (online/offline), the chat service publishes
/// a `PresenceEvent` to this channel. This background task receives those events
/// and pushes `WsMessage::PresenceUpdate` to every connected client on THIS
/// service instance. Clients are expected to filter updates for users they follow.
///
/// Scalability note: at large scale, replace with per-user sharded channels and
/// a follower-graph cache so each instance only receives events for its users.
pub struct PresenceSubscriber {
    connections: ConnectionRegistry,
    redis_url: String,
}

impl PresenceSubscriber {
    pub fn new(connections: ConnectionRegistry, redis_url: String) -> Self {
        Self {
            connections,
            redis_url,
        }
    }

    pub async fn run(&self) -> AppResult<()> {
        info!("Presence subscriber starting");

        let client = redis::Client::open(self.redis_url.as_str())
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;

        let conn = client
            .get_async_connection()
            .await
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;
        let mut pubsub = conn.into_pubsub();

        pubsub
            .subscribe(crate::service::chat_service::PRESENCE_CHANNEL)
            .await
            .map_err(|e| shared::errors::AppError::Cache(e.to_string()))?;

        info!(
            channel = crate::service::chat_service::PRESENCE_CHANNEL,
            "Subscribed to presence channel"
        );

        let mut stream = pubsub.on_message();

        loop {
            match stream.next().await {
                Some(msg) => {
                    let payload: String = match msg.get_payload() {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(error = %e, "Failed to read presence pub/sub payload");
                            continue;
                        }
                    };

                    match serde_json::from_str::<PresenceEvent>(&payload) {
                        Ok(event) => self.broadcast_presence(event),
                        Err(e) => {
                            warn!(error = %e, payload = %payload, "Failed to parse PresenceEvent")
                        }
                    }
                }
                None => {
                    error!("Presence pub/sub stream ended unexpectedly");
                    return Err(shared::errors::AppError::Cache(
                        "Pub/sub stream closed".to_string(),
                    ));
                }
            }
        }
    }

    /// Fan out presence update to all locally-connected WebSocket clients.
    fn broadcast_presence(&self, event: PresenceEvent) {
        let ws_msg = WsMessage::PresenceUpdate {
            user_id: event.user_id,
            online: event.online,
            last_seen: Some(event.last_seen),
        };

        let mut delivered = 0usize;
        for entry in self.connections.iter() {
            // Don't echo back to the user whose status just changed.
            if *entry.key() == event.user_id {
                continue;
            }
            if entry.value().send(ws_msg.clone()).is_ok() {
                delivered += 1;
            }
        }

        tracing::debug!(
            user_id = %event.user_id,
            online = event.online,
            delivered_to = delivered,
            "Presence update broadcast"
        );
    }
}

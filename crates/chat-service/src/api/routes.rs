use std::sync::Arc;

use axum::extract::ws::{Message as WsFrame, WebSocket};
use axum::{
    extract::{Path, Query, RawQuery, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use shared::{
    auth::extract_token_from_query,
    errors::{AppError, AppResult},
    models::message::WsMessage,
    observability::health_check,
};

use crate::{domain::models::CreateConversationRequest, AppState};

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        // WebSocket endpoint — token passed as ?token=<jwt> (browsers can't set headers)
        .route("/api/v1/chat/ws", get(ws_upgrade_handler))
        .route(
            "/api/v1/chat/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/api/v1/chat/conversations/:id/messages", get(get_messages))
        .route("/api/v1/chat/conversations/:id/read", post(mark_read))
        .with_state(state)
}

// ── WebSocket ──────────────────────────────────────────────────────────────────

/// HTTP upgrade handler. Authenticates via JWT in query string, then hands
/// off to the WebSocket session handler.
async fn ws_upgrade_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    RawQuery(query): RawQuery,
) -> impl IntoResponse {
    let query_str = query.unwrap_or_default();
    let claims = extract_token_from_query(&query_str)
        .ok_or_else(|| AppError::Unauthorized("Missing token query parameter".to_string()))
        .and_then(|t| state.jwt_service.validate_token(&t));

    match claims {
        Ok(claims) => {
            let user_id = claims.sub;
            ws.on_upgrade(move |socket| handle_socket(socket, state, user_id))
                .into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}

/// Runs for the lifetime of a single WebSocket connection.
///
/// Architecture:
///   - Each connection gets an mpsc channel (tx stays in `connections` registry,
///     rx is drained by a `send_task` that forwards to the WebSocket write half).
///   - The receive loop handles incoming WsMessage frames from the client.
///   - On disconnect (clean or error), the connection is removed from the registry
///     and the presence service is notified.
#[instrument(skip(socket, state), fields(user_id = %user_id))]
async fn handle_socket(socket: WebSocket, state: Arc<AppState>, user_id: Uuid) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();

    // Register — other service calls can now push messages to this connection.
    state.connections.insert(user_id, tx);
    metrics::gauge!("websocket_connections_active").increment(1.0);

    // Mark user online in Redis.
    if let Err(e) = state.chat_service.set_presence_online(user_id).await {
        warn!(user_id = %user_id, error = %e, "Failed to set presence online");
    }

    info!("WebSocket connected");

    // Forward outbound messages (from mpsc rx) to the WebSocket write half.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if ws_tx.send(WsFrame::Text(text)).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to serialize WsMessage — dropping");
                }
            }
        }
    });

    // Process inbound frames from the WebSocket.
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(WsFrame::Text(text)) => match serde_json::from_str::<WsMessage>(&text) {
                Ok(msg) => dispatch_ws_message(msg, user_id, &state).await,
                Err(e) => warn!(error = %e, "Unparseable WsMessage from client"),
            },
            Ok(WsFrame::Close(_)) | Err(_) => break,
            _ => {} // Binary / Ping frames ignored — clients use WsMessage::Ping
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────────────
    state.connections.remove(&user_id);
    metrics::gauge!("websocket_connections_active").decrement(1.0);
    send_task.abort();

    // Mark user offline — publish presence change to Redis so other instances
    // can push PresenceUpdate to followers' WebSocket connections.
    if let Err(e) = state.chat_service.set_presence_offline(user_id).await {
        warn!(user_id = %user_id, error = %e, "Failed to set presence offline");
    }

    info!("WebSocket disconnected");
}

/// Route an inbound WsMessage to the appropriate service method.
async fn dispatch_ws_message(msg: WsMessage, user_id: Uuid, state: &Arc<AppState>) {
    match msg {
        WsMessage::SendMessage {
            id: idempotency_key,
            conversation_id,
            content,
            message_type,
        } => {
            metrics::counter!("messages_sent_total").increment(1);

            match state
                .chat_service
                .send_message(
                    user_id,
                    conversation_id,
                    content,
                    message_type,
                    idempotency_key,
                )
                .await
            {
                Ok((message, seq)) => {
                    // Echo delivery confirmation back to sender.
                    if let Some(tx) = state.connections.get(&user_id) {
                        let _ = tx.send(WsMessage::Delivered {
                            message_id: message.id,
                            sequence_number: seq,
                        });
                    }
                }
                Err(e) => {
                    error!(user_id = %user_id, error = %e, "send_message failed");
                    if let Some(tx) = state.connections.get(&user_id) {
                        let _ = tx.send(WsMessage::Error {
                            code: "SEND_FAILED".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        WsMessage::Ack { message_id } => {
            let _ = state.chat_service.mark_delivered(message_id).await;
        }

        WsMessage::Ping => {
            if let Some(tx) = state.connections.get(&user_id) {
                let _ = tx.send(WsMessage::Pong);
            }
        }

        // Server-only variants; clients should not send these.
        _ => warn!(user_id = %user_id, "Received unexpected WsMessage variant from client"),
    }
}

// ── REST endpoints ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MessageQuery {
    limit: Option<i64>,
    before_sequence: Option<i64>,
}

#[derive(Deserialize)]
struct ConversationQuery {
    limit: Option<i64>,
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ConversationQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    let limit = q.limit.unwrap_or(20).min(50);
    let convs = state.chat_service.get_conversations(user_id, limit).await?;
    Ok(Json(serde_json::json!({ "data": convs, "limit": limit })))
}

async fn create_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateConversationRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    if user_id == req.participant_id {
        return Err(AppError::BadRequest(
            "Cannot start a conversation with yourself".to_string(),
        ));
    }
    let conv = state
        .chat_service
        .get_or_create_conversation(user_id, req.participant_id)
        .await?;
    Ok(Json(serde_json::to_value(&conv).unwrap()))
}

async fn get_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
    Query(q): Query<MessageQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    let limit = q.limit.unwrap_or(50).min(100);

    // Authorization: only participants may read messages.
    let conv = state
        .chat_service
        .get_conversation(conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;
    if conv.participant_a != user_id && conv.participant_b != user_id {
        return Err(AppError::Forbidden(
            "Not a participant in this conversation".to_string(),
        ));
    }

    let messages = state
        .chat_service
        .get_messages(conversation_id, q.before_sequence, limit)
        .await?;

    let has_more = messages.len() as i64 == limit;
    let next_cursor = messages.last().map(|m| m.sequence_number);

    Ok(Json(serde_json::json!({
        "data": messages,
        "limit": limit,
        "has_more": has_more,
        "next_cursor": next_cursor,
    })))
}

async fn mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = extract_user_id(&headers)?;
    state
        .chat_service
        .mark_read(conversation_id, user_id)
        .await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

fn extract_user_id(headers: &HeaderMap) -> AppResult<Uuid> {
    headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid X-User-Id header".to_string()))
}

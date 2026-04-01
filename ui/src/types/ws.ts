import type { Message, MessageType } from "./api";

// Mirrors the WsMessage enum in shared/src/models/message.rs

export type WsMessage =
  | { type: "send_message"; id: string; conversation_id: string; content: string; message_type: MessageType }
  | { type: "new_message"; message: Message }
  | { type: "ack"; message_id: string }
  | { type: "delivered"; message_id: string; sequence_number: number }
  | { type: "ping" }
  | { type: "pong" }
  | { type: "presence_update"; user_id: string; online: boolean; last_seen: string | null }
  | { type: "error"; code: string; message: string };

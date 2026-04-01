import { useCallback, useEffect, useRef, useState } from "react";
import { v4 as uuidv4 } from "uuid";
import { useAuthStore } from "@/stores/auth";
import { chatApi } from "@/lib/api";
import type { Message } from "@/types/api";
import type { WsMessage } from "@/types/ws";

export type WsStatus = "connecting" | "connected" | "reconnecting" | "disconnected";

interface PendingMessage {
  idempotencyKey: string;
  conversationId: string;
  content: string;
  localId: string; // optimistic UI id
}

interface UseChatOptions {
  conversationId: string | null;
  onMessage: (msg: Message) => void;
  onPresenceUpdate?: (userId: string, online: boolean, lastSeen: string | null) => void;
}

export function useChat({ conversationId, onMessage, onPresenceUpdate }: UseChatOptions) {
  const token = useAuthStore((s) => s.token);
  const [status, setStatus] = useState<WsStatus>("disconnected");

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectAttempt = useRef(0);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pingTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const pongTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSeenSeq = useRef<number>(0);
  const pending = useRef<Map<string, PendingMessage>>(new Map());
  const mountedRef = useRef(true);

  // Backoff: 1s · 2s · 4s · 8s · 16s · 30s (cap), plus jitter.
  const backoffMs = (attempt: number) =>
    Math.min(1000 * 2 ** attempt, 30_000) + Math.random() * 1_000;

  const stopTimers = useCallback(() => {
    if (pingTimer.current) clearInterval(pingTimer.current);
    if (pongTimeout.current) clearTimeout(pongTimeout.current);
    if (reconnectTimer.current) clearTimeout(reconnectTimer.current);
  }, []);

  const send = useCallback((msg: WsMessage) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  }, []);

  const connect = useCallback(() => {
    if (!token || !mountedRef.current) return;
    if (wsRef.current) {
      wsRef.current.onclose = null; // prevent reconnect loop on intentional close
      wsRef.current.close();
    }

    setStatus("connecting");
    // Vite dev proxy rewrites /ws → ws://localhost:8084/api/v1/chat/ws
    const wsUrl =
      import.meta.env.DEV
        ? `/ws?token=${token}`
        : `${location.protocol === "https:" ? "wss" : "ws"}://${location.host.replace(/:\d+/, ":8084")}/api/v1/chat/ws?token=${token}`;

    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      if (!mountedRef.current) return;
      reconnectAttempt.current = 0;
      setStatus("connected");

      // Recover missed messages for the active conversation.
      if (conversationId && lastSeenSeq.current > 0) {
        chatApi
          .messages(conversationId, { before_sequence: lastSeenSeq.current + 1, limit: 50 })
          .then((page) => {
            // Deliver missed messages in order (API returns newest-first, so reverse).
            [...page.data].reverse().forEach((m) => {
              if (m.sequence_number > lastSeenSeq.current) {
                onMessage(m);
                lastSeenSeq.current = Math.max(lastSeenSeq.current, m.sequence_number);
              }
            });
          })
          .catch(() => {}); // best-effort
      }

      // Retry un-acked pending messages with the same idempotency keys.
      for (const p of pending.current.values()) {
        send({
          type: "send_message",
          id: p.idempotencyKey,
          conversation_id: p.conversationId,
          content: p.content,
          message_type: "text",
        });
      }

      // Ping every 30s, expect pong within 10s.
      pingTimer.current = setInterval(() => {
        send({ type: "ping" });
        pongTimeout.current = setTimeout(() => {
          ws.close(); // triggers onclose → reconnect
        }, 10_000);
      }, 30_000);
    };

    ws.onmessage = (event) => {
      let msg: WsMessage;
      try {
        msg = JSON.parse(event.data as string) as WsMessage;
      } catch {
        return;
      }

      switch (msg.type) {
        case "new_message":
          onMessage(msg.message);
          lastSeenSeq.current = Math.max(lastSeenSeq.current, msg.message.sequence_number);
          send({ type: "ack", message_id: msg.message.id });
          break;

        case "delivered":
          pending.current.delete(msg.message_id);
          break;

        case "pong":
          if (pongTimeout.current) clearTimeout(pongTimeout.current);
          break;

        case "presence_update":
          onPresenceUpdate?.(msg.user_id, msg.online, msg.last_seen);
          break;

        case "error":
          console.error(`[ws] server error ${msg.code}: ${msg.message}`);
          break;
      }
    };

    ws.onerror = () => {
      // onclose always fires after onerror — handle reconnect there.
    };

    ws.onclose = () => {
      if (!mountedRef.current) return;
      stopTimers();
      setStatus("reconnecting");
      const delay = backoffMs(reconnectAttempt.current++);
      reconnectTimer.current = setTimeout(() => {
        if (mountedRef.current) connect();
      }, delay);
    };
  }, [token, conversationId, onMessage, onPresenceUpdate, send, stopTimers]);

  // Expose sendMessage so pages don't need to construct WsMessage manually.
  const sendMessage = useCallback(
    (conversationId: string, content: string): string => {
      const idempotencyKey = uuidv4();
      const localId = uuidv4();

      pending.current.set(idempotencyKey, { idempotencyKey, conversationId, content, localId });

      send({
        type: "send_message",
        id: idempotencyKey,
        conversation_id: conversationId,
        content,
        message_type: "text",
      });

      return localId; // returned so caller can show optimistic UI
    },
    [send],
  );

  useEffect(() => {
    mountedRef.current = true;
    if (token) connect();
    return () => {
      mountedRef.current = false;
      stopTimers();
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.close();
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  return { status, sendMessage };
}

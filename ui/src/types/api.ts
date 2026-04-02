// ── Users ─────────────────────────────────────────────────────────────────────

export interface User {
  id: string;
  username: string;
  email: string;
  display_name: string | null;
  bio: string | null;
  avatar_url: string | null;
  follower_count: number;
  following_count: number;
  created_at: string;
}

export interface AuthResponse {
  access_token: string;
  token_type: string;
  expires_in: number;
  user_id: string;
  username: string;
}

// ── Posts ─────────────────────────────────────────────────────────────────────

export interface Post {
  id: string;
  author_id: string;
  content: string;
  media_urls: string[];
  tags: string[];
  like_count: number;
  created_at: string;
  // Hydrated client-side from the user cache
  author?: User;
}

export interface CreatePostRequest {
  content: string;
  media_urls?: string[];
  tags?: string[];
}

// ── Feed ──────────────────────────────────────────────────────────────────────

export interface FeedItem {
  post_id: string;
  score: number; // timestamp_ms — used as cursor
}

export interface FeedPage {
  items: FeedItem[];
  next_cursor: number | null;
  has_more: boolean;
}

// ── Chat ──────────────────────────────────────────────────────────────────────

export type MessageType = "text" | "image" | "video" | "file" | "system";

export interface Message {
  id: string;
  conversation_id: string;
  sender_id: string;
  content: string;
  message_type: MessageType;
  sequence_number: number;
  idempotency_key: string;
  delivered_at: string | null;
  read_at: string | null;
  created_at: string;
}

export interface Conversation {
  id: string;
  participant_a: string;
  participant_b: string;
  last_message_id: string | null;
  last_message_at: string | null;
  created_at: string;
  // Hydrated client-side
  other_user?: User;
  last_message?: Message;
}

export interface MessagesPage {
  data: Message[];
  limit: number;
  has_more: boolean;
  next_cursor: number | null;
}

// ── Presence ─────────────────────────────────────────────────────────────────

export interface PresenceStatus {
  user_id: string;
  online: boolean;
  last_seen: string;
}

// ── API error shape ───────────────────────────────────────────────────────────

export interface ApiError {
  error: {
    code: string;
    message: string;
  };
}

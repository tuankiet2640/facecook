import axios, { type AxiosError } from "axios";
import { useAuthStore } from "@/stores/auth";

// Axios instance — all API calls go through here so auth + error handling
// are applied exactly once, not scattered across hooks.
export const apiClient = axios.create({
  baseURL: "/api/v1",
  headers: { "Content-Type": "application/json" },
  timeout: 15_000,
});

// Inject Bearer token on every request.
apiClient.interceptors.request.use((config) => {
  const token = useAuthStore.getState().token;
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// On 401: clear auth state so the user is redirected to /auth.
apiClient.interceptors.response.use(
  (res) => res,
  (err: AxiosError) => {
    if (err.response?.status === 401) {
      useAuthStore.getState().clear();
    }
    return Promise.reject(err);
  },
);

// ── Auth ──────────────────────────────────────────────────────────────────────

import type { AuthResponse, User } from "@/types/api";

export const authApi = {
  register: (data: { username: string; email: string; password: string; display_name?: string }) =>
    apiClient.post<AuthResponse>("/auth/register", data).then((r) => r.data),

  login: (data: { username: string; password: string }) =>
    apiClient.post<AuthResponse>("/auth/login", data).then((r) => r.data),
};

// ── Users ─────────────────────────────────────────────────────────────────────

export const usersApi = {
  me: () => apiClient.get<User>("/users/me").then((r) => r.data),

  get: (userId: string) => apiClient.get<User>(`/users/${userId}`).then((r) => r.data),

  update: (data: { display_name?: string; bio?: string; avatar_url?: string }) =>
    apiClient.put<User>("/users/me", data).then((r) => r.data),

  follow: (userId: string) =>
    apiClient.post(`/users/${userId}/follow`).then((r) => r.data),

  unfollow: (userId: string) =>
    apiClient.delete(`/users/${userId}/unfollow`).then((r) => r.data),

  followers: (userId: string, limit = 20, offset = 0) =>
    apiClient.get<{ data: User[]; limit: number; offset: number }>(
      `/users/${userId}/followers`,
      { params: { limit, offset } },
    ).then((r) => r.data),

  following: (userId: string, limit = 20, offset = 0) =>
    apiClient.get<{ data: User[]; limit: number; offset: number }>(
      `/users/${userId}/following`,
      { params: { limit, offset } },
    ).then((r) => r.data),
};

// ── Posts ─────────────────────────────────────────────────────────────────────

import type { Post, CreatePostRequest } from "@/types/api";

export const postsApi = {
  create: (data: CreatePostRequest) =>
    apiClient.post<Post>("/posts", data).then((r) => r.data),

  get: (postId: string) => apiClient.get<Post>(`/posts/${postId}`).then((r) => r.data),

  batch: (ids: string[]) =>
    apiClient
      .get<{ data: Post[] }>("/posts/batch", { params: { ids: ids.join(",") } })
      .then((r) => r.data.data),

  delete: (postId: string) =>
    apiClient.delete(`/posts/${postId}`).then((r) => r.data),
};

// ── Feed ──────────────────────────────────────────────────────────────────────

import type { FeedPage } from "@/types/api";

export const feedApi = {
  get: (params: { limit?: number; before_score?: number }) =>
    apiClient.get<FeedPage>("/feed", { params }).then((r) => r.data),
};

// ── Chat ──────────────────────────────────────────────────────────────────────

import type { Conversation, MessagesPage } from "@/types/api";

export const chatApi = {
  conversations: (limit = 20) =>
    apiClient.get<{ data: Conversation[]; limit: number }>("/chat/conversations", { params: { limit } })
      .then((r) => r.data.data),

  createConversation: (participantId: string) =>
    apiClient.post<Conversation>("/chat/conversations", { participant_id: participantId })
      .then((r) => r.data),

  messages: (conversationId: string, params: { limit?: number; before_sequence?: number } = {}) =>
    apiClient.get<MessagesPage>(`/chat/conversations/${conversationId}/messages`, { params })
      .then((r) => r.data),

  markRead: (conversationId: string) =>
    apiClient.post(`/chat/conversations/${conversationId}/read`).then((r) => r.data),
};

// ── Presence ─────────────────────────────────────────────────────────────────

import type { PresenceStatus } from "@/types/api";

export const presenceApi = {
  get: (userId: string) =>
    apiClient.get<PresenceStatus>(`/presence/${userId}`).then((r) => r.data),

  batch: (userIds: string[]) =>
    apiClient.post<{ data: PresenceStatus[] }>("/presence/batch", { user_ids: userIds })
      .then((r) => r.data.data),
};

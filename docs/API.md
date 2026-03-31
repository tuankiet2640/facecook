# API Reference

Base URL: `https://api.facecook.io` (production) / `http://localhost:8080` (local)

All authenticated endpoints require `Authorization: Bearer <jwt>` header.
Protected routes at the gateway strip the `Authorization` header and inject `X-User-Id`.

---

## Authentication

### POST /api/v1/auth/register
Register a new user account.

**Request:**
```json
{
  "username": "alice",
  "email": "alice@example.com",
  "password": "supersecret123",
  "display_name": "Alice"
}
```

**Response 200:**
```json
{
  "access_token": "eyJhbGciOiJIUzI1NiJ9...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "user_id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice"
}
```

### POST /api/v1/auth/login
```json
{ "username": "alice", "password": "supersecret123" }
```
Returns the same shape as register.

---

## Users

### GET /api/v1/users/me
Returns the authenticated user's full profile.

### PUT /api/v1/users/me
Update profile fields (display_name, bio, avatar_url).

### GET /api/v1/users/:user_id
Returns a public profile (no private fields).

### GET /api/v1/users/username/:username
Look up user by username.

### POST /api/v1/users/:user_id/follow
Follow a user. Idempotent — following an already-followed user is a no-op.

**Response:** `{ "success": true }`

### DELETE /api/v1/users/:user_id/unfollow

### GET /api/v1/users/:user_id/followers?limit=20&offset=0

### GET /api/v1/users/:user_id/following?limit=20&offset=0

---

## Posts

### POST /api/v1/posts
Create a post. Triggers async feed fanout via Kafka.

**Request:**
```json
{
  "content": "Hello Facecook! #rust #distributed-systems",
  "media_urls": [],
  "tags": ["rust", "distributed-systems"]
}
```

**Response 201:**
```json
{
  "id": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
  "author_id": "550e8400...",
  "content": "Hello Facecook!",
  "created_at": "2026-03-31T10:00:00Z"
}
```

### GET /api/v1/posts/:post_id
### DELETE /api/v1/posts/:post_id
Soft-deletes (sets `is_deleted = true`).

### GET /api/v1/posts/batch?ids=id1,id2,id3
Fetch multiple posts in a single round-trip (used by feed service to hydrate post content).

---

## Feed

### GET /api/v1/feed?limit=20&before_score=1711871400000.0
Retrieve the authenticated user's personalized feed.

**Pagination:** cursor-based using `before_score` (timestamp_ms of the last item from the previous page).

**Response:**
```json
{
  "items": [
    { "post_id": "uuid", "score": 1711871399000.0 },
    { "post_id": "uuid", "score": 1711871350000.0 }
  ],
  "next_cursor": 1711871350000.0,
  "has_more": true
}
```

The feed returns post UUIDs + scores only. Clients fetch full post content from the Post Service using the `/api/v1/posts/batch` endpoint. This separation keeps feed reads extremely fast (pure Redis) and avoids coupling feed freshness to post content.

---

## Chat

### GET /api/v1/chat/conversations?limit=20
List conversations ordered by last_message_at DESC.

### POST /api/v1/chat/conversations
Get or create a 1:1 conversation.
```json
{ "participant_id": "uuid-of-other-user" }
```

### GET /api/v1/chat/conversations/:id/messages?limit=50&before_sequence=100
Paginated message history, newest first. Cursor = `before_sequence`.

### POST /api/v1/chat/conversations/:id/read
Mark all unread messages in conversation as read.

### WebSocket: ws://localhost:8084/api/v1/chat/ws?token=<jwt>
See [Realtime.md](Realtime.md) for full WebSocket protocol.

---

## Presence

### GET /api/v1/presence/:user_id
```json
{ "user_id": "uuid", "online": true, "last_seen": "2026-03-31T10:05:00Z" }
```

### POST /api/v1/presence/batch
```json
{ "user_ids": ["uuid1", "uuid2", "uuid3"] }
```
Returns array of PresenceStatus for each user found.

---

## Error Format

All errors follow a consistent envelope:
```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "User not found"
  }
}
```

| HTTP Status | Code | Meaning |
|---|---|---|
| 400 | BAD_REQUEST | Invalid input |
| 401 | UNAUTHORIZED | Missing/invalid JWT |
| 403 | FORBIDDEN | Authenticated but not authorized |
| 404 | NOT_FOUND | Resource not found |
| 409 | CONFLICT | Duplicate (e.g., username taken) |
| 422 | VALIDATION_ERROR | Failed field validation |
| 429 | RATE_LIMITED | Too many requests |
| 502 | BAD_GATEWAY | Upstream service unavailable |
| 504 | GATEWAY_TIMEOUT | Upstream timed out |
| 500 | INTERNAL_ERROR | Unexpected server error |

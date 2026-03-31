-- Celebrity post index.
--
-- Used by the fanout-on-read path. When a high-follower (celebrity) user creates
-- a post, the post is recorded here instead of being pushed to every follower's
-- feed sorted set. At read time, this table is consulted to merge celebrity posts
-- into the requesting user's feed.
--
-- Durability: Redis sorted sets are volatile; this table is the durable fallback
-- if the Redis key expires or the instance restarts.
CREATE TABLE celebrity_posts (
    post_id     UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    author_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- timestamp_ms is the Redis sorted-set score; stored here for consistent ordering.
    timestamp_ms BIGINT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (author_id, post_id)
);

-- "Give me all celebrity posts from author X, newest first" — used on cache miss.
CREATE INDEX celebrity_posts_author_ts ON celebrity_posts (author_id, timestamp_ms DESC);

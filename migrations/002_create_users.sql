-- Users table.
--
-- Sharding note: user_id (UUID v4) is the natural shard key for all user-centric
-- data. If sharding PostgreSQL (e.g. Citus), partition by HASH(user_id).
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username        TEXT NOT NULL,
    email           TEXT NOT NULL,
    password_hash   TEXT NOT NULL,
    display_name    TEXT,
    bio             TEXT,
    avatar_url      TEXT,
    -- follower_count cached here to avoid expensive COUNT(*) on hot follow paths.
    -- Kept eventually consistent via increment/decrement on follow/unfollow events.
    follower_count  BIGINT NOT NULL DEFAULT 0,
    following_count BIGINT NOT NULL DEFAULT 0,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    is_verified     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Unique constraints enforce identity invariants.
CREATE UNIQUE INDEX users_username_unique ON users (LOWER(username));
CREATE UNIQUE INDEX users_email_unique    ON users (LOWER(email));

-- Trigram indexes for LIKE/ILIKE search on username and display_name.
CREATE INDEX users_username_trgm ON users USING GIN (username gin_trgm_ops);
CREATE INDEX users_display_name_trgm ON users USING GIN (display_name gin_trgm_ops);

-- updated_at auto-maintenance trigger.
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

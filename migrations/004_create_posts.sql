-- Posts table.
--
-- Partitioning strategy: RANGE partition by created_at (monthly buckets).
-- Old partitions can be detached and archived to cold storage.
-- Alternatively, partition by HASH(author_id) for write locality.
CREATE TABLE posts (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    author_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    media_urls  JSONB NOT NULL DEFAULT '[]',
    -- Tags stored as a normalized array for fast containment queries.
    tags        TEXT[] NOT NULL DEFAULT '{}',
    like_count  BIGINT NOT NULL DEFAULT 0,
    -- Ranking score — initially set to created_at epoch; can be updated by ML ranking.
    -- Using a separate column (not just timestamp) makes ranking extensible.
    rank_score  DOUBLE PRECISION NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW()),
    is_deleted  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Primary read patterns:
-- 1. Fetch post by id (primary key)
-- 2. Fetch all posts by author (profile page, used by fanout-on-read for celebrities)
-- 3. Fetch recent posts (admin / trending feed)
CREATE INDEX posts_author_created ON posts (author_id, created_at DESC)
    WHERE is_deleted = FALSE;
CREATE INDEX posts_rank_score     ON posts (rank_score DESC)
    WHERE is_deleted = FALSE;
-- GIN index for tag queries (e.g. WHERE tags @> '{#rust}')
CREATE INDEX posts_tags ON posts USING GIN (tags);

CREATE TRIGGER posts_updated_at
    BEFORE UPDATE ON posts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

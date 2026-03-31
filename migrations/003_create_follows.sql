-- Follow graph.
--
-- Design: adjacency list (follower_id, followee_id). Both columns are indexed
-- to support O(1) follow/unfollow and efficient follower/following list queries.
--
-- At large scale (billions of edges) this moves to a graph store (e.g., JanusGraph)
-- or a dedicated social graph service backed by a NoSQL store.
CREATE TABLE follows (
    follower_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    followee_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (follower_id, followee_id),
    -- Self-follows are not allowed.
    CONSTRAINT no_self_follow CHECK (follower_id != followee_id)
);

-- "Who follows user X?" — used by fanout-on-write to enumerate followers.
CREATE INDEX follows_followee_id ON follows (followee_id, created_at DESC);
-- "Who does user X follow?" — used by feed build to identify followed celebrities.
CREATE INDEX follows_follower_id ON follows (follower_id, created_at DESC);

-- Maintain cached counts on users table atomically.
CREATE OR REPLACE FUNCTION update_follow_counts()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE users SET follower_count  = follower_count  + 1 WHERE id = NEW.followee_id;
        UPDATE users SET following_count = following_count + 1 WHERE id = NEW.follower_id;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE users SET follower_count  = GREATEST(follower_count  - 1, 0) WHERE id = OLD.followee_id;
        UPDATE users SET following_count = GREATEST(following_count - 1, 0) WHERE id = OLD.follower_id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER follows_update_counts
    AFTER INSERT OR DELETE ON follows
    FOR EACH ROW EXECUTE FUNCTION update_follow_counts();

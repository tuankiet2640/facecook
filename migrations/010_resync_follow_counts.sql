-- Resync follower_count / following_count from the source-of-truth `follows`
-- table.
--
-- Why this exists: an earlier version of user_repo::follow() ran manual
-- UPDATE statements *in addition* to the trigger in 003_create_follows.sql,
-- which double-incremented the cached counts on every follow. Commit 8f5a986
-- removed the redundant UPDATEs, but databases that ran the old code already
-- have inflated counts. New follows will increment correctly, but the
-- historical drift never heals on its own — this migration corrects it once.
--
-- Idempotent: re-running has no effect because COUNT(*) on the follows table
-- is the source of truth.

UPDATE users u
SET follower_count = sub.cnt
FROM (
    SELECT followee_id AS user_id, COUNT(*)::bigint AS cnt
    FROM follows
    GROUP BY followee_id
) sub
WHERE u.id = sub.user_id
  AND u.follower_count <> sub.cnt;

UPDATE users u
SET follower_count = 0
WHERE u.follower_count <> 0
  AND NOT EXISTS (SELECT 1 FROM follows f WHERE f.followee_id = u.id);

UPDATE users u
SET following_count = sub.cnt
FROM (
    SELECT follower_id AS user_id, COUNT(*)::bigint AS cnt
    FROM follows
    GROUP BY follower_id
) sub
WHERE u.id = sub.user_id
  AND u.following_count <> sub.cnt;

UPDATE users u
SET following_count = 0
WHERE u.following_count <> 0
  AND NOT EXISTS (SELECT 1 FROM follows f WHERE f.follower_id = u.id);

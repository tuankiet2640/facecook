-- Add post_visibility enum type
CREATE TYPE post_visibility AS ENUM ('public', 'friends', 'private');

-- Add missing columns to posts
ALTER TABLE posts
    ADD COLUMN visibility     post_visibility NOT NULL DEFAULT 'public',
    ADD COLUMN comment_count  BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN share_count    BIGINT NOT NULL DEFAULT 0;

-- Add post_count cache column to users
ALTER TABLE users
    ADD COLUMN post_count BIGINT NOT NULL DEFAULT 0;

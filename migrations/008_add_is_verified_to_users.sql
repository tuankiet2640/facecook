-- Add is_verified flag to users table
ALTER TABLE users
    ADD COLUMN is_verified BOOLEAN NOT NULL DEFAULT FALSE;

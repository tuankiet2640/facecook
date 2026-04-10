-- Make display_name NOT NULL with an empty-string default so it maps to
-- String (not Option<String>) in the Rust User model.
UPDATE users SET display_name = '' WHERE display_name IS NULL;
ALTER TABLE users
    ALTER COLUMN display_name SET NOT NULL,
    ALTER COLUMN display_name SET DEFAULT '';

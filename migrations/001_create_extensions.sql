-- Enable UUID generation and pg_trgm for full-text search on usernames/emails.
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

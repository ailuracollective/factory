-- Migration 0003: replace plaintext runner tokens with hashed verifiers.
-- Existing runner tokens are invalidated on purpose; runners must re-register.
DROP TABLE IF EXISTS runners;

CREATE TABLE runners (
    id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    rotated_at INTEGER,
    revoked_at INTEGER
);

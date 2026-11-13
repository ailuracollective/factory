-- Migration 0002: execution policies per repository
CREATE TABLE IF NOT EXISTS policies (
    repo TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1,
    allowed_users TEXT NOT NULL DEFAULT '[]',
    max_concurrent INTEGER NOT NULL DEFAULT 2,
    daily_quota INTEGER NOT NULL DEFAULT 20,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS policy_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    repo TEXT NOT NULL,
    actor TEXT,
    decision TEXT NOT NULL,
    reason TEXT
);

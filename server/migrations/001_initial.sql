-- server/migrations/001_initial.sql
CREATE TABLE IF NOT EXISTS submissions (
    id           TEXT PRIMARY KEY,
    url          TEXT NOT NULL,
    filename     TEXT,
    status       TEXT NOT NULL DEFAULT 'pending',
    submitted_at TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    id           TEXT PRIMARY KEY,
    key_hash     TEXT NOT NULL,
    label        TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);

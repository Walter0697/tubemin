CREATE TABLE IF NOT EXISTS shortcut_setup_tokens (
    id            TEXT PRIMARY KEY,
    token_hash    TEXT NOT NULL,
    owner_sub     TEXT,
    owner_display TEXT NOT NULL,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_shortcut_setup_tokens_expires_at
    ON shortcut_setup_tokens (expires_at);

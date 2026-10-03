CREATE TABLE IF NOT EXISTS service_accounts (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    token_hash    TEXT NOT NULL,
    scopes_json   TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    managed_by    TEXT NOT NULL DEFAULT 'manifest',
    last_used_at  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transfers (
    id                    TEXT PRIMARY KEY,
    submission_id         TEXT NOT NULL,
    peertube_uuid         TEXT NOT NULL,
    service_account_id    TEXT NOT NULL,
    consumer              TEXT NOT NULL,
    destination           TEXT NOT NULL,
    destination_ref       TEXT,
    source_title          TEXT,
    source_url            TEXT,
    state                 TEXT NOT NULL,
    output_size           INTEGER,
    source_cleanup_state  TEXT NOT NULL DEFAULT 'pending',
    peertube_delete_state TEXT NOT NULL DEFAULT 'pending',
    error                 TEXT,
    retry_count           INTEGER NOT NULL DEFAULT 0,
    created_at            TEXT NOT NULL,
    completed_at          TEXT,
    deleted_at            TEXT,
    updated_at            TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS transfers_peertube_uuid_idx
    ON transfers(peertube_uuid);

CREATE INDEX IF NOT EXISTS transfers_service_account_idx
    ON transfers(service_account_id);

CREATE INDEX IF NOT EXISTS transfers_state_idx
    ON transfers(state);

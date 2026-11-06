ALTER TABLE submissions ADD COLUMN peertube_next_retry_at TEXT;
ALTER TABLE submissions ADD COLUMN peertube_upload_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE submissions ADD COLUMN peertube_last_error TEXT;

CREATE INDEX IF NOT EXISTS submissions_peertube_queue_idx
    ON submissions(status, peertube_uuid, peertube_next_retry_at);

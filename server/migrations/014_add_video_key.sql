ALTER TABLE submissions ADD COLUMN video_key TEXT;

CREATE INDEX IF NOT EXISTS submissions_video_key_idx
    ON submissions(video_key);

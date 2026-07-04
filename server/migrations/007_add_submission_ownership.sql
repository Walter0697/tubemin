ALTER TABLE api_keys ADD COLUMN owner_sub TEXT;
ALTER TABLE api_keys ADD COLUMN owner_display TEXT;

ALTER TABLE submissions ADD COLUMN api_key_id TEXT;
ALTER TABLE submissions ADD COLUMN submitter_sub TEXT;
ALTER TABLE submissions ADD COLUMN submitter_display TEXT;
ALTER TABLE submissions ADD COLUMN submitter_tag TEXT;

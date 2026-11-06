# PeerTube Upload Queue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn TubeMin's PeerTube import directory into a continuously retryable, rate-limit-aware upload queue.

**Architecture:** Keep media files in `/peertube-import` and add SQLite retry metadata to submissions. Reuse the existing upload function and token cache from a serial background worker that shares the existing upload lock with new-file and handoff paths. Startup recovery and periodic processing use the same queue scan.

**Tech Stack:** Rust, Tokio, Axum service, SQLx SQLite migrations, Reqwest, existing PeerTube client.

## Global Constraints

- Preserve existing `imported`, `transcoding`, `complete`, `error`, and handoff semantics for existing rows.
- Never delete a staged media file after PeerTube upload; downstream migration owns cleanup.
- Process at most one PeerTube upload at a time.
- Reuse the existing cached PeerTube token.
- Keep all existing tests passing.

---

### Task 1: Add durable retry metadata

**Files:**
- Create: `server/migrations/015_add_peertube_upload_queue.sql`
- Modify: `server/src/db.rs`
- Test: `server/src/db.rs` existing async test module

- [ ] Add nullable `peertube_next_retry_at`, integer `peertube_upload_attempts` defaulting to zero, and nullable `peertube_last_error` columns to `submissions`.
- [ ] Add database helpers to select due staged submissions by filename, claim an upload attempt, schedule a retry, and clear queue metadata after success.
- [ ] Write tests proving due rows are selected, future rows are skipped, and stale `uploading` attempts can be reclaimed.
- [ ] Run the focused database tests and then the full test suite.
- [ ] Commit as `feat: add durable PeerTube upload queue metadata`.

### Task 2: Implement retry scheduling

**Files:**
- Modify: `server/src/watcher.rs`

- [ ] Add a small retry-delay helper that parses `Retry-After` seconds and applies bounded exponential backoff when no header is available.
- [ ] Write failing unit tests for header delay, fallback delay, and maximum delay.
- [ ] Add a queue scan that reads staged video files, resolves matching submissions, skips rows with a PeerTube UUID, and skips rows whose next retry time is in the future.
- [ ] Reuse the existing metadata, subtitle, upload, and UUID persistence logic for a queued attempt.
- [ ] On success clear queue metadata; on failure persist the error and next retry time without removing the staged file.
- [ ] Run focused watcher tests and the full test suite.
- [ ] Commit as `feat: add rate-aware PeerTube retry scheduling`.

### Task 3: Run the queue continuously and recover startup state

**Files:**
- Modify: `server/src/watcher.rs`
- Modify: `server/src/main.rs`

- [ ] Replace the one-shot startup-only import retry with the shared queue processor.
- [ ] Start a Tokio interval worker that scans the import directory continuously and wakes immediately at startup.
- [ ] Ensure stale attempts are made eligible on startup and that the worker cannot overlap with the normal watcher or handoff upload path.
- [ ] Preserve the existing skip behavior for no-audio/video files and already-handed-off submissions.
- [ ] Add a test for a failed queued file becoming eligible without a process restart.
- [ ] Run the full Rust test suite and `git diff --check`.
- [ ] Commit as `feat: continuously process PeerTube import queue`.

### Task 4: Verify and publish

**Files:**
- No additional source files.

- [ ] Run `cargo test` from `server/` and confirm zero failures.
- [ ] Confirm the migration applies to a fresh SQLite database and existing rows remain readable.
- [ ] Inspect the final diff for unrelated changes.
- [ ] Push the commits directly to `origin/main`.
- [ ] Report the exact commit, test result, and deployment requirement.

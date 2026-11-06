# PeerTube Upload Queue Design

## Goal

Make TubeMin's PeerTube import durable and continuously retryable while retaining staged media for downstream migration.

## Current problem

TubeMin moves completed media into `/peertube-import`, marks the submission `imported`, and attempts the PeerTube upload inline. If that upload fails, the file remains staged but the only retry is the next process startup. The `imported` state therefore does not distinguish a downloaded file waiting for PeerTube from a successfully uploaded video.

## Design

The import directory remains the durable queue and source of truth for media files. A background worker periodically scans it, finds video files whose submissions do not yet have a PeerTube UUID, and processes them serially under the existing upload lock. The startup recovery scan becomes one invocation of the same queue-processing logic rather than a separate one-shot behavior.

Each attempt reuses TubeMin's cached PeerTube access token. Rate-limit responses are treated as scheduled retries: the worker honors `Retry-After` when available and otherwise uses bounded exponential backoff for transient HTTP, network, and PeerTube errors. Permanent media errors, such as files without both audio and video streams, remain skipped and are logged.

The database gains queue metadata for the next retry time, attempt count, and last error. Existing `imported` rows remain compatible; queue metadata is added without rewriting historical status values. A successful upload stores the PeerTube UUID and thumbnail exactly as today. The original staged file remains in `/peertube-import` for downstream migration.

The worker resets stale in-progress attempts on startup, processes at most one upload at a time, and wakes on a short interval so new failures recover without a restart. It does not delete media or alter ConvertTube handoff records.

## Error handling

- HTTP 429: schedule at the server-provided `Retry-After`, with a bounded fallback delay.
- Network and 5xx errors: exponential backoff with a maximum delay.
- Invalid media: log and leave the file untouched for operator review.
- Missing database match: leave the file untouched and log it.
- Successful PeerTube upload: persist UUID/thumbnail and clear queue error metadata.

## Testing

- Unit-test retry-delay calculation for `Retry-After`, fallback backoff, and the maximum delay.
- Test queue selection ignores files with a PeerTube UUID and respects `next_retry_at`.
- Test stale in-progress queue entries are recoverable after restart.
- Preserve and run all existing watcher, database, and PeerTube tests.

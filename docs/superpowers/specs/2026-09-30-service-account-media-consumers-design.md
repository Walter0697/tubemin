# Tubemin Service Accounts and Remote Media Consumers

## Status

Design approved for implementation planning. The first consumer is ConvertTube;
Jellypik will migrate to the same contract later.

## Goal

Allow remote services to consume videos owned by Tubemin without sharing a
filesystem or PeerTube credentials. ConvertTube should be able to list
Tubemin-owned PeerTube videos, retrieve the original source media, convert it
into the Navidrome library, and report successful completion so Tubemin can
clean up the source safely.

## System boundary

```text
ConvertTube --service account--> Tubemin --PeerTube bot--> PeerTube
                                      |
                                      +-- catalog
                                      +-- original media streaming
                                      +-- transfer history
                                      +-- cleanup/deletion
```

Tubemin remains the owner of the PeerTube service account and the authority for
PeerTube deletion. ConvertTube and Jellypik receive only Tubemin credentials.
User-facing API keys remain for browser extensions and personal submissions;
they are not used as machine-to-machine identities.

## Service-account model

Tubemin will support system-level service accounts defined by a startup
manifest and persisted in SQLite for runtime authentication and history.

Example manifest:

```toml
[[service_accounts]]
name = "converttube"
token_env = "TUBEMIN_CONVERTTUBE_TOKEN"
scopes = [
  "catalog:read",
  "media:read",
  "transfer:complete",
  "transfer:fail",
]
enabled = true
```

Startup reconciliation will:

1. Read the manifest.
2. Resolve tokens from environment variables or Docker secrets.
3. Hash tokens and store only the hashes.
4. Create or update declared accounts in SQLite.
5. Apply declared scopes and enabled state.

The manifest will not store plaintext tokens in Git. Missing manifest entries
will not automatically delete accounts. Runtime records will include enabled
state, last-used time, and audit identity.

The PeerTube bot credentials remain internal Tubemin configuration. They are
not exposed to service accounts.

## API contract

All endpoints below require the appropriate service-account scope.

```text
GET  /api/service/catalog
GET  /api/service/videos/:uuid
GET  /api/service/videos/:uuid/media
GET  /api/service/videos/:uuid/thumbnail
POST /api/service/videos/:uuid/complete
POST /api/service/videos/:uuid/fail
```

### Catalog

The catalog returns active videos owned by Tubemin's PeerTube service account.
Deleted or completed items are excluded by default. Items include the PeerTube
UUID, title, description, duration, source URL where appropriate, thumbnail
URL, published time, processing state, and original-media availability.

### Metadata and media

Media is addressed by PeerTube UUID, never by an arbitrary filesystem path or
external URL. Tubemin verifies ownership and resolves the corresponding
submission before streaming media.

The media endpoint prefers the original local source artifact when available;
otherwise it streams PeerTube's original/download source. It should preserve
the original filename and media type and support range requests where
possible. The thumbnail endpoint proxies PeerTube thumbnails through Tubemin,
so consumers do not need PeerTube credentials or direct PeerTube reachability.

### Completion

ConvertTube reports successful output creation with a payload like:

```json
{
  "consumer": "converttube",
  "destination": "navidrome",
  "output_path": "/music/Artist/Album/song.mp3",
  "output_size": 12345678
}
```

Tubemin validates ownership and state, creates a transfer record, removes its
local source artifacts, deletes the PeerTube video using the internal bot
account, and records the final result. Completion is idempotent.

The failure endpoint records the failure and leaves the PeerTube source
available for retry. A consumer may also leave the source untouched by not
calling completion.

## Lifecycle and cleanup

The intended lifecycle is:

```text
active -> processing -> completed -> cleanup_pending -> deleted
```

Failures remain visible and retryable. PeerTube `404 Not Found` during a
repeat cleanup is treated as already deleted. If PeerTube deletion fails, the
record remains `cleanup_pending`; it is not removed from the database.

Tubemin should preserve the original submission row with a terminal `deleted`
state rather than physically deleting it. This prevents duplicate processing,
preserves provenance, and supports troubleshooting.

## Persistence and history

### Service accounts

The `service_accounts` model will contain an ID, name, token hash, scopes,
enabled state, manifest-management marker, last-used time, and timestamps.

### Transfers

A separate transfer record will contain:

- PeerTube UUID;
- Tubemin submission ID;
- service-account ID and consumer name;
- destination type and reference/path;
- source title and URL snapshot;
- lifecycle state;
- output metadata;
- error and retry information;
- completion and cleanup timestamps.

Tubemin will provide a transfer-history page showing consumer, destination,
state, timestamps, source identity, and cleanup errors. This will become the
central history for ConvertTube and, later, Jellypik.

## ConvertTube behavior

ConvertTube will retain its existing conversion behavior:

1. List source items in its left panel.
2. Download a selected original source to a local temporary location.
3. Convert to MP3, normalize volume, apply tags/art, and place output in the
   Navidrome library.
4. Call Tubemin completion only after successful output creation.
5. Remove the local temporary source after success.

Failed conversion leaves the PeerTube source available for retry.

## Jellypik compatibility

Jellypik will not be changed in the first implementation. Later it will receive
its own Tubemin service account and report JellyFin transfers through the same
completion and cleanup contract. Its direct PeerTube deletion and shared
cleanup-token path can then be retired.

## Security

- Service accounts use dedicated credentials and least-privilege scopes.
- PeerTube credentials never leave Tubemin.
- Catalog, media, thumbnail, completion, and failure requests are audited by
  service-account identity.
- Ownership is checked before catalog access, media retrieval, or cleanup.
- Tubemin will not proxy arbitrary external URLs.
- Completion and cleanup operations are idempotent.
- Plaintext service tokens are supplied through deployment secrets and are not
  persisted in SQLite.

## Rollout and verification

### Phase 1: Tubemin

- Implement manifest-defined service accounts and scoped authentication.
- Add catalog, metadata, media, thumbnail, completion, and failure endpoints.
- Add transfer records, cleanup states, and history UI.
- Preserve existing user API-key behavior and Jellypik endpoints.

### Phase 2: ConvertTube

- Replace the MeTube/local-file source list with the Tubemin catalog.
- Use Tubemin thumbnail and media endpoints.
- Reuse existing conversion, normalization, tagging, and Navidrome output.
- Report completion only after successful output creation.

### Phase 3: Jellypik

- Add a dedicated Jellypik service account.
- Replace direct PeerTube deletion with Tubemin completion/cleanup.
- Record JellyFin transfers in the shared Tubemin history.

Tests must cover manifest initialization, token hashing, scope enforcement,
catalog filtering, ownership validation, original media streaming, successful
completion, failure preservation, idempotent completion, cleanup retries,
transfer history, and Jellypik-compatible completion requests.

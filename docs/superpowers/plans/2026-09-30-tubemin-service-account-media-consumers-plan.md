# Tubemin Service Account Media Consumers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add manifest-defined Tubemin service accounts and a scoped remote API so ConvertTube can list Tubemin-owned PeerTube videos, stream original sources, report results, and trigger auditable cleanup without receiving PeerTube credentials.

**Architecture:** Tubemin reconciles service-account definitions from TOML into SQLite, authenticates machine clients with scoped bcrypt-backed tokens, and exposes `/api/service`. The existing Tubemin PeerTube bot remains the only PeerTube credential holder. Successful completion creates transfer history, cleans source artifacts, deletes PeerTube media, and preserves the submission row.

**Tech Stack:** Rust 2021, Axum 0.7, Tokio, SQLx SQLite migrations, bcrypt, Reqwest, `toml`, Minijinja, `axum-test`, `wiremock`.

## Global Constraints

- Preserve existing user API-key behavior for browser extensions and shortcuts.
- Never expose PeerTube credentials to ConvertTube or Jellypik.
- Only Tubemin-owned PeerTube videos are eligible for service catalog, media, or cleanup operations.
- Persist only bcrypt token hashes; never store plaintext service tokens in SQLite or Git.
- The manifest defines deployment accounts; SQLite stores runtime authentication, usage, transfers, and audit history.
- Failed consumer work leaves the PeerTube source available for retry.
- Cleanup is idempotent; PeerTube `404 Not Found` is already-deleted success.
- Media is addressed by an owned PeerTube UUID, never an arbitrary filesystem path or external URL.
- This plan implements Tubemin only. ConvertTube integration and Jellypik migration are later plans.

## File map

### New files

- `server/migrations/013_add_service_accounts_and_transfers.sql` — service-account and transfer schema.
- `server/src/service_accounts.rs` — manifest parsing, reconciliation, scopes, token verification, extractor.
- `server/src/handlers/service_api.rs` — catalog, metadata, media, thumbnail, completion, failure routes.
- `server/src/handlers/transfers.rs` — authenticated transfer-history page/API.
- `server/templates/transfers.html` — transfer-history markup.
- `server/static/transfers.js` — transfer-history loading/rendering.

### Modified files

- `server/Cargo.toml`, `server/src/config.rs`, `server/src/db.rs`, `server/src/state.rs`, `server/src/main.rs`.
- `server/src/peertube.rs` — original-source and thumbnail operations.
- `server/src/handlers/mod.rs`, `server/src/handlers/internal_handoff.rs`.
- `server/templates/partials/nav.html`, `example.env`, `docker-compose.yml`, `README.md`.

---

### Task 1: Add service-account and transfer persistence

**Files:**
- Create: `server/migrations/013_add_service_accounts_and_transfers.sql`
- Modify: `server/src/db.rs`
- Test: `server/src/db.rs` inline tests

**Interfaces:** Produces `ServiceAccountRow`, `TransferRow`, and persistence helpers used by authentication and service handlers.

- [ ] **Step 1: Write failing persistence tests**

Add inline tests for idempotent account synchronization, transfer creation,
preserving a deleted submission row, and excluding deleted submissions from the
active catalog.

The idempotency test applies the same account definition twice and asserts that
the account table contains one row. The history test completes a transfer,
then asserts that its original submission row and transfer row are both still
present.

- [ ] **Step 2: Add the migration**

Create `service_accounts` with `id`, unique `name`, `token_hash`,
`scopes_json`, `enabled`, `managed_by`, `last_used_at`, and timestamps.

Create `transfers` with `id`, `submission_id`, `peertube_uuid`,
`service_account_id`, `consumer`, `destination`, `destination_ref`, source
title/URL snapshots, `state`, output metadata, independent source-cleanup and
PeerTube-delete states, error, retry count, and timestamps. Add indexes on
PeerTube UUID, service account, and transfer state.

- [ ] **Step 3: Implement typed database helpers**

Add helpers with these signatures or equivalent established project types:

```rust
pub async fn upsert_service_account(pool: &SqlitePool, account: &ServiceAccountRow) -> Result<(), sqlx::Error>;
pub async fn find_service_account_by_name(pool: &SqlitePool, name: &str) -> Result<Option<ServiceAccountRow>, sqlx::Error>;
pub async fn list_service_accounts(pool: &SqlitePool) -> Result<Vec<ServiceAccountRow>, sqlx::Error>;
pub async fn touch_service_account(pool: &SqlitePool, id: &str, now: &str) -> Result<(), sqlx::Error>;
pub async fn active_catalog_submissions(pool: &SqlitePool) -> Result<Vec<Submission>, sqlx::Error>;
pub async fn create_transfer(pool: &SqlitePool, transfer: &TransferRow) -> Result<(), sqlx::Error>;
pub async fn get_transfer_for_video(pool: &SqlitePool, uuid: &str, account_id: &str) -> Result<Option<TransferRow>, sqlx::Error>;
pub async fn update_transfer_cleanup(pool: &SqlitePool, id: &str, state: &str, error: Option<&str>) -> Result<(), sqlx::Error>;
pub async fn list_transfers(pool: &SqlitePool) -> Result<Vec<TransferRow>, sqlx::Error>;
```

Add guarded submission transitions for `processing`, `completed`,
`cleanup_pending`, and `deleted`. Do not physically delete source submission
rows during consumer cleanup.

- [ ] **Step 4: Run focused tests**

Run `cd server && cargo test db::tests -- --nocapture`.
Expected: existing database tests and new persistence tests pass.

- [ ] **Step 5: Commit**

```bash
git add server/migrations/013_add_service_accounts_and_transfers.sql server/src/db.rs
git commit -m "feat: persist service accounts and transfer history"
```

### Task 2: Add manifest reconciliation and scoped authentication

**Files:**
- Modify: `server/Cargo.toml`, `server/src/config.rs`, `server/src/state.rs`, `server/src/main.rs`
- Create: `server/src/service_accounts.rs`
- Test: `server/src/service_accounts.rs` and `server/src/config.rs` inline tests

**Interfaces:** Produces `ServiceAccountManifest`, `ServicePrincipal`,
`ServiceScope`, `RequireServiceAccount`, and
`sync_manifest(pool: &SqlitePool, path: &Path) -> Result<(), anyhow::Error>`.

- [ ] **Step 1: Write failing manifest/auth tests**

Test TOML parsing, missing-token rejection, duplicate-name rejection, bcrypt
hashing, idempotent sync, disabled-account rejection, invalid-token rejection,
and missing-scope rejection.

The synchronization test applies a manifest twice and asserts one database row
with a bcrypt hash that does not equal the plaintext token. The authentication
test asserts that a disabled account returns `401` and that an enabled account
without the requested scope returns `403`.

- [ ] **Step 2: Add configuration and TOML types**

Add the `toml` dependency and `TUBEMIN_SERVICE_ACCOUNTS_FILE`, defaulting to
`/data/service-accounts.toml`. Define:

```rust
pub struct ServiceAccountDefinition {
    pub name: String,
    pub token_env: String,
    pub scopes: Vec<ServiceScope>,
    pub enabled: bool,
}
```

Validate duplicate/empty names, unknown scopes, missing environment variables,
and empty tokens before changing the database.

- [ ] **Step 3: Implement reconciliation and extractor**

Use bcrypt to hash manifest tokens and store JSON scopes. Reconcile declared
accounts without deleting accounts absent from the file. Define:

```rust
pub struct ServicePrincipal { pub id: String, pub name: String, pub scopes: HashSet<ServiceScope> }
pub struct RequireServiceAccount { pub principal: ServicePrincipal }
impl RequireServiceAccount { pub fn require(&self, scope: ServiceScope) -> Result<(), Response>; }
```

Authenticate `Authorization: Bearer <token>`, touch `last_used_at`, return
`401` for invalid credentials and `403` for missing scopes, and keep this path
separate from the user API-key verifier.

- [ ] **Step 4: Wire startup synchronization and test**

Call `service_accounts::sync_manifest(&pool, &config.service_accounts_file)` after database initialization and
before serving requests. Invalid configured manifests fail startup; an absent
manifest preserves current startup behavior and logs that no service accounts
are configured. Run:

```bash
cd server
cargo test service_accounts config -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add server/Cargo.toml server/src/config.rs server/src/service_accounts.rs server/src/state.rs server/src/main.rs
git commit -m "feat: add manifest-defined service accounts"
```

### Task 3: Extend the PeerTube client and source-artifact handling

**Files:**
- Modify: `server/src/peertube.rs`, `server/src/db.rs`
- Modify: `server/src/handlers/internal_handoff.rs`
- Test: `server/src/peertube.rs` inline tests and client tests

**Interfaces:** Produces typed account listing, video detail, original-source,
thumbnail, and delete operations for `service_api`.

- [ ] **Step 1: Write failing `wiremock` tests**

Cover paginated account listing, original-source URL resolution, thumbnail
fetching, and PeerTube `404` mapping for idempotent deletion.

- [ ] **Step 2: Add typed PeerTube operations**

Reuse the existing access-token and `Host` override code. Expose operations
equivalent to:

```rust
pub async fn list_account_videos(url: &str, host: Option<&str>, username: &str, password: &str) -> Result<Vec<ListedVideo>>;
pub async fn get_video(url: &str, host: Option<&str>, username: &str, password: &str, uuid: &str) -> Result<PeerTubeVideoDetail>;
pub async fn stream_original(url: &str, host: Option<&str>, username: &str, password: &str, uuid: &str) -> Result<reqwest::Response>;
pub async fn fetch_thumbnail(url: &str, host: Option<&str>, username: &str, password: &str, uuid: &str) -> Result<(String, Vec<u8>)>;
pub async fn delete_video(url: &str, host: Option<&str>, username: &str, password: &str, uuid: &str) -> Result<DeleteOutcome>;
```

Keep original/source files distinct from PeerTube-generated playback files.

- [ ] **Step 3: Centralize safe source cleanup**

Extract filename validation, sidecar enumeration, and `/downloads` plus
`/peertube-import` cleanup from `internal_handoff.rs` into a shared helper.
Keep the existing Jellypik handoff endpoint behavior unchanged while making it
use the helper.

- [ ] **Step 4: Run tests and commit**

Run `cd server && cargo test peertube -- --nocapture`, then:

```bash
git add server/src/peertube.rs server/src/db.rs server/src/handlers/internal_handoff.rs
git commit -m "feat: expose original PeerTube media operations"
```

### Task 4: Implement catalog, metadata, media, and thumbnail endpoints

**Files:**
- Create: `server/src/handlers/service_api.rs`
- Modify: `server/src/handlers/mod.rs`, `server/src/main.rs`
- Test: `server/src/handlers/service_api.rs` inline tests

**Interfaces:** Consumes Tasks 1–3 and produces the read endpoints used by
ConvertTube.

- [ ] **Step 1: Write failing endpoint tests**

Using `axum-test` and `wiremock`, test service-scope enforcement, active
catalog filtering, exclusion of unknown PeerTube videos, ownership rejection,
local-source preference, PeerTube-source fallback, range/stream headers, and
thumbnail proxying.

- [ ] **Step 2: Implement stable response types and catalog**

Register:

```rust
.route("/api/service/catalog", get(handlers::service_catalog))
.route("/api/service/videos/:uuid", get(handlers::service_video))
```

Intersect PeerTube bot-account inventory with active Tubemin submissions. Do
not expose arbitrary PeerTube account videos.

- [ ] **Step 3: Implement media and thumbnail routes**

Register:

```rust
.route("/api/service/videos/:uuid/media", get(handlers::service_media))
.route("/api/service/videos/:uuid/thumbnail", get(handlers::service_thumbnail))
```

Verify ownership and submission lookup before every response. Stream local
original files with `ReaderStream` when available; otherwise proxy the original
PeerTube source without buffering the whole file. Set safe media type,
filename, length when known, and range behavior. Derive thumbnail upstream
URLs from the verified record, never from client input.

- [ ] **Step 4: Run endpoint tests and commit**

Run `cd server && cargo test handlers::service_api -- --nocapture`, then:

```bash
git add server/src/handlers/service_api.rs server/src/handlers/mod.rs server/src/main.rs
git commit -m "feat: add remote service catalog and media API"
```

### Task 5: Implement completion, failure, and cleanup state transitions

**Files:**
- Modify: `server/src/handlers/service_api.rs`, `server/src/db.rs`
- Test: `server/src/handlers/service_api.rs` and `server/src/db.rs` inline tests

**Interfaces:** Consumes service authentication and UUID lookup; produces
durable transfer state and cleanup results.

- [ ] **Step 1: Write failing completion tests**

Cover successful deletion, repeated completion, another service-account
rejection, failed PeerTube deletion leaving `cleanup_pending`, local cleanup
failure retention, and failure recording without source deletion.

The idempotency test calls completion twice and asserts one transfer record,
one deletion request, and a successful second response. The failure test
asserts that the submission remains active and no PeerTube deletion request is
made.

- [ ] **Step 2: Implement completion**

Register:

```rust
.route("/api/service/videos/:uuid/complete", post(handlers::service_complete))
```

Require `transfer:complete`. Validate ownership and active state, create or
reuse one transfer row, snapshot consumer/destination data, and transition the
submission through `processing`, `completed`, and `cleanup_pending`.

Remove local artifacts, call PeerTube deletion, and update source-cleanup and
PeerTube-delete states independently. Mark the submission `deleted` only when
PeerTube is confirmed deleted. Treat `404` as success and retain artifact
cleanup errors for retry/history.

- [ ] **Step 3: Implement failure reporting**

Register:

```rust
.route("/api/service/videos/:uuid/fail", post(handlers::service_fail))
```

Require `transfer:fail`, record the error and retry metadata, and leave the
source active. Never call PeerTube deletion from this route.

- [ ] **Step 4: Run tests and commit**

Run `cd server && cargo test db::tests handlers::service_api -- --nocapture`,
then:

```bash
git add server/src/handlers/service_api.rs server/src/db.rs
git commit -m "feat: add audited service transfer cleanup"
```

### Task 6: Add transfer history for human Tubemin users

**Files:**
- Create: `server/src/handlers/transfers.rs`, `server/templates/transfers.html`, `server/static/transfers.js`
- Modify: `server/src/handlers/mod.rs`, `server/src/main.rs`, `server/templates/partials/nav.html`
- Test: `server/src/handlers/transfers.rs` inline tests

**Interfaces:** Consumes transfer rows and produces authenticated `/transfers`
and `/api/transfers` views.

- [ ] **Step 1: Write failing history tests**

Test that browser-session authentication is required and that responses include
consumer, destination, state, timestamps, source identity, output reference,
and cleanup errors without exposing secrets.

- [ ] **Step 2: Implement API and page**

Use the existing `RequireAuth` pattern. Show newest transfers first and add
filters for consumer and state. Clearly expose `cleanup_pending` and failed
items; this first version does not perform cleanup actions.

- [ ] **Step 3: Run tests and commit**

Run `cd server && cargo test handlers::transfers -- --nocapture`, then commit:

```bash
git add server/src/handlers/transfers.rs server/templates/transfers.html server/static/transfers.js server/templates/partials/nav.html server/src/handlers/mod.rs server/src/main.rs
git commit -m "feat: add transfer history page"
```

### Task 7: Document deployment and verify the complete Tubemin implementation

**Files:**
- Modify: `example.env`, `docker-compose.yml`, `README.md`
- Test: full server suite and manual API smoke test

- [ ] **Step 1: Document deployment configuration**

Document `TUBEMIN_SERVICE_ACCOUNTS_FILE`, the TOML shape, token environment
references, supported scopes, and the requirement that secrets come from
deployment configuration rather than Git.

- [ ] **Step 2: Add a safe local Compose mount**

Mount a service-account configuration directory into persistent `/data` without
adding real credentials or plaintext tokens to the repository.

- [ ] **Step 3: Document the API lifecycle**

Add catalog, media, completion, failure, and lifecycle examples to the README.
Document that ConvertTube is the first consumer and Jellypik will later migrate
from direct PeerTube deletion.

- [ ] **Step 4: Run full verification**

Run:

```bash
cd server
cargo fmt --check
cargo test
```

Then verify with a configured test service account:

```bash
curl -fsS -H "Authorization: Bearer $TUBEMIN_CONVERTTUBE_TOKEN" \
  "$TUBEMIN_URL/api/service/catalog"
```

Confirm active owned videos are listed, thumbnails/media require the service
credential, completion creates history, and cleanup is idempotent.

- [ ] **Step 5: Commit**

```bash
git add example.env docker-compose.yml README.md
git commit -m "docs: document service consumer deployment"
```

## Final verification checklist

- [ ] Existing user API-key tests pass unchanged.
- [ ] Existing Jellypik handoff and cleanup-token endpoints remain compatible.
- [ ] Plaintext service tokens are never persisted.
- [ ] Catalog contains only active Tubemin-owned videos.
- [ ] Original media streams without a shared filesystem.
- [ ] Failed ConvertTube work leaves PeerTube untouched.
- [ ] Successful completion creates transfer history and deletes idempotently.
- [ ] Deleted submissions remain in history and are hidden from the active catalog.
- [ ] Cleanup failures remain visible and retryable.
- [ ] ConvertTube and Jellypik code remain unchanged by this Tubemin-only plan.

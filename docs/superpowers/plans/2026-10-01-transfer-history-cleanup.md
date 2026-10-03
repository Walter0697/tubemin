# Transfer History Cleanup Implementation Plan

> Superseded by the simpler dashboard-filtering design: transfer history is
> retained permanently, and Dashboard excludes `deleted` and legacy
> `handed_off` submissions. No cleanup endpoint or button is implemented.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an authenticated Tubemin Transfers-page action that permanently removes only new `deleted` transfer rows and legacy Jellypik `handed_off` submission rows after confirmation.

**Architecture:** Add one transactional database operation that deletes the two archive categories and returns separate counts. Expose it through an authenticated POST route, then add a Transfers-page button/modal that appears only when archive rows exist and refreshes the table after success. No PeerTube, filesystem, or Navidrome operation is performed.

The existing transfer-list response is extended with archive counts from both
the `transfers` and `submissions` tables, because legacy `handed_off`
submissions are not represented by transfer rows.

**Tech Stack:** Rust, Axum, SQLx/SQLite, Minijinja, vanilla JavaScript, existing Tubemin dark-theme CSS, Docker Compose.

## Global Constraints

- Delete only `transfers.state = 'deleted'` and `submissions.status = 'handed_off'`.
- Leave completed, failed, pending, active, and all other submission states untouched.
- Require the existing authenticated Tubemin session for the page and endpoint.
- The confirmation must state that only local history rows are removed; PeerTube videos, source files, and Navidrome output are unaffected.
- Keep the implementation on the existing `feature/tubemin-service-account-media-consumers` branch.

---

### Task 1: Add the transactional database cleanup

**Files:**
- Modify: `server/src/db.rs` near `list_transfers`
- Test: `server/src/db.rs` existing SQLite test module

**Interfaces:**
- Produces `pub struct ArchivedTransferCleanup { pub deleted_transfers: u64, pub handed_off_submissions: u64 }`.
- Produces `pub async fn clear_archived_transfer_history(pool: &SqlitePool) -> Result<ArchivedTransferCleanup, sqlx::Error>`.
- Produces `pub async fn count_archived_transfer_history(pool: &SqlitePool) -> Result<(i64, i64), sqlx::Error>` for current archive counts.

- [ ] **Step 1: Write the failing database test**

Create an in-memory database fixture containing one `transfers` row in each of these states: `deleted`, `completed`, and `failed`; and one `submissions` row in each of these statuses: `handed_off`, `completed`, and `failed`. Call `clear_archived_transfer_history` and assert the result is one deleted transfer and one handed-off submission. Query the remaining rows and assert all non-archive rows still exist.

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cd /home/git/video-download-stack/tubemin/server
cargo test clear_archived_transfer_history -- --nocapture
```

Expected: compilation fails because the cleanup result type and function do not exist.

- [ ] **Step 3: Implement the transactional operation**

Use `pool.begin().await?`, execute these constrained deletes in order, record `rows_affected()` for each, and commit only after both succeed:

```rust
let deleted_transfers = sqlx::query("DELETE FROM transfers WHERE state = 'deleted'")
    .execute(&mut *tx)
    .await?
    .rows_affected();
let handed_off_submissions = sqlx::query(
    "DELETE FROM submissions WHERE status = 'handed_off'",
)
    .execute(&mut *tx)
    .await?
    .rows_affected();
```

Return both counts in `ArchivedTransferCleanup`.

- [ ] **Step 4: Run the focused test and verify it passes**

Run the same `cargo test clear_archived_transfer_history -- --nocapture` command and expect PASS.

- [ ] **Step 5: Commit the database change**

```bash
git add server/src/db.rs
git commit -m "feat: add archived transfer history cleanup"
```

### Task 2: Expose an authenticated cleanup endpoint

**Files:**
- Modify: `server/src/handlers/transfers.rs`
- Modify: `server/src/main.rs`
- Test: `server/src/handlers/transfers.rs` existing handler tests

**Interfaces:**
- Adds `pub async fn clear_archived_transfers(RequireAuth(_user): RequireAuth, State(state): State<AppState>) -> impl IntoResponse`.
- Adds POST route `/api/transfers/cleanup-archived`.
- Returns `{ "deletedTransfers": number, "handedOffSubmissions": number }` on success and HTTP 500 with `{ "error": "archived transfer cleanup failed" }` on database failure.
- Extends `GET /api/transfers` with `archiveCounts: { deletedTransfers, handedOffSubmissions }`.

- [ ] **Step 1: Add handler tests for the success response and route protection**

Cover that the handler returns both count fields and that an unauthenticated request cannot invoke it. Follow the existing `transfers.rs` test setup and authentication conventions rather than creating a new auth mechanism.

- [ ] **Step 2: Run the focused handler tests and verify the new test fails**

Run:

```bash
cd /home/git/video-download-stack/tubemin/server
cargo test transfers -- --nocapture
```

Expected: the new test fails because the route and handler are not registered.

- [ ] **Step 3: Implement and register the handler**

Call `db::clear_archived_transfer_history(&state.pool)`, serialize the two counts using camelCase JSON keys, and map database errors to HTTP 500. Register the route in the existing authenticated `api_routes` router next to `/transfers`.

- [ ] **Step 4: Run focused handler tests**

Run `cargo test transfers -- --nocapture` and expect PASS.

- [ ] **Step 5: Commit the endpoint change**

```bash
git add server/src/handlers/transfers.rs server/src/main.rs
git commit -m "feat: expose archived transfer cleanup endpoint"
```

### Task 3: Add the Transfers-page button and confirmation flow

**Files:**
- Modify: `server/templates/transfers.html`
- Modify: `server/static/transfers.js`
- Modify: `server/static/style.css`

**Interfaces:**
- The existing `/api/transfers` response retains its `transfers` array and adds `archiveCounts`.
- The page adds a `Clear archived records` button with an accessible confirmation modal.
- The page POSTs to `/api/transfers/cleanup-archived` only after confirmation.

- [ ] **Step 1: Add the UI markup and failing browser-level smoke assertion**

Add an initially hidden archive toolbar/button and confirmation modal to `transfers.html`. The modal copy must say: “This removes deleted transfer history and legacy handed-off history only. PeerTube videos, source files, and Navidrome output are not affected.” Add a lightweight script assertion or existing frontend test fixture that fails until the button is rendered for archive rows.

- [ ] **Step 2: Implement archive counting and rendering**

In `transfers.js`, use the API-provided archive counts, render the button only when either count is nonzero, and include separate counts in the button/confirmation text.

- [ ] **Step 3: Implement confirmation and request flow**

On confirmation, POST JSON `{}` to `/api/transfers/cleanup-archived`; on success, close the modal, show the removed counts, and reload the transfer list; on failure, leave the list visible and show an error message. Do not call any PeerTube or media endpoint from this flow.

- [ ] **Step 4: Add dark-theme styling**

Style the toolbar/button/modal using existing Tubemin tokens such as `--surface`, `--surface-2`, `--border`, `--text`, `--text-muted`, and the danger/status colors. Preserve the current horizontal table scrolling and mobile layout.

- [ ] **Step 5: Run frontend/static checks**

Run:

```bash
cd /home/git/video-download-stack/tubemin
node --check server/static/transfers.js
git diff --check
```

No ConvertTube files should change.

- [ ] **Step 6: Commit the Transfers-page UI**

```bash
git add server/templates/transfers.html server/static/transfers.js server/static/style.css
git commit -m "feat: add transfer history archive cleanup UI"
```

### Task 4: Full verification and local container rollout

**Files:**
- Verify: `server/src/db.rs`, `server/src/handlers/transfers.rs`, `server/src/main.rs`, `server/templates/transfers.html`, `server/static/transfers.js`, `server/static/style.css`

- [ ] **Step 1: Run the complete Tubemin test suite**

Run:

```bash
cd /home/git/video-download-stack/tubemin/server
cargo test
```

Expected: all tests pass.

- [ ] **Step 2: Build and restart the local Tubemin container**

Run from `/home/git/video-download-stack/tubemin`:

```bash
docker compose -f docker-compose.yml up -d --build tubemin
```

Use the base compose file so Tubemin remains exposed on port 7005 and does not conflict with ConvertTube on port 3000.

- [ ] **Step 3: Verify the live API and page**

Open [http://localhost:7005/transfers](http://localhost:7005/transfers), verify the archive button/count appears for existing `deleted` or `handed_off` rows, cancel once to confirm no mutation, then confirm once and verify the rows disappear while other states remain.

- [ ] **Step 4: Verify container health and repository state**

Run `docker compose -f docker-compose.yml ps tubemin`, `git diff --check`, and `git status --short --branch`. Confirm Tubemin is healthy and the feature branch contains only the intended commits.

- [ ] **Step 5: Commit any final verification-only documentation if needed**

Do not modify production configuration or perform PeerTube deletion as part of this feature. If no documentation change is needed, leave the worktree clean after the implementation commits.

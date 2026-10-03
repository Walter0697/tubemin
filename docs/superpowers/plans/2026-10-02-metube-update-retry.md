# MeTube Update Retry Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stop Tubemin from repeatedly updating/restarting MeTube after a failed download has exhausted its retry budget.

**Architecture:** Reorder the existing poller flow so the database atomically claims a retry before any optional MeTube update request. The existing retry counter remains the source of truth; no schema change or new service is needed.

**Tech Stack:** Rust, Tokio, SQLx SQLite, reqwest, existing async unit tests.

## Global Constraints

- Keep MeTube update recovery opt-in through the existing `METUBE_AUTO_UPDATE_ON_FAILURE` setting.
- Do not change ConvertTube, PeerTube, or the MeTube container.
- Do not update MeTube when `claim_metube_retry` returns false.
- Preserve the existing maximum retry budget of three.

### Task 1: Add the regression test

**Files:**
- Modify: `server/src/db.rs` test module near `metube_retry_budget_is_three`

**Interfaces:**
- Consumes: existing `claim_metube_retry` database helper.
- Produces: a failing regression test showing that an exhausted item cannot be claimed again.

- [ ] **Step 1: Add an explicit exhausted-retry test**

Extend the existing retry-budget coverage with a test named `metube_retry_claim_is_false_after_budget_exhaustion`. Create an errored submission, claim it three times while returning it to `error` between claims, then assert that a fourth claim is false and the stored `download_retries` remains three.

- [ ] **Step 2: Run the focused test**

Run: `cargo test metube_retry_claim_is_false_after_budget_exhaustion -- --nocapture`

Expected: PASS against the existing database helper. This test locks down the boundary used by the poller before changing the poller ordering.

### Task 2: Reorder the poller update flow

**Files:**
- Modify: `server/src/poller.rs:80-135`

**Interfaces:**
- Consumes: `db::claim_metube_retry`, existing update/wait/submit helpers.
- Produces: retry processing that only invokes MeTube update recovery after a retry claim succeeds.

- [ ] **Step 1: Move retry claiming before update recovery**

Inside the `state.errored` loop, call `claim_metube_retry` before checking `auto_update_on_failure`. Keep the existing `Ok(false) => {}` behavior with no update request, no MeTube submission, and no warning loop. Put the optional update request, readiness wait, and `metube::submit` inside the `Ok(true)` branch.

- [ ] **Step 2: Preserve update failure behavior**

If the update request fails or MeTube does not become ready, continue with the already-claimed retry and submit it using the existing normal retry path. If submission fails, retain the current `mark_pending_as_error_by_url` behavior.

- [ ] **Step 3: Run focused tests**

Run: `cargo test metube_retry -- --nocapture`

Expected: all retry-related tests pass.

### Task 3: Verify and commit

**Files:**
- Verify: `server/src/poller.rs`
- Verify: `server/src/db.rs`

- [ ] **Step 1: Run formatting and the full test suite**

Run: `cargo fmt --check`

Run: `cargo test`

Expected: formatting passes and all tests pass.

- [ ] **Step 2: Review the diff**

Run: `git diff --check` and `git diff -- server/src/poller.rs server/src/db.rs`

Confirm that only the retry ordering and regression coverage changed.

- [ ] **Step 3: Commit on the feature branch**

Run: `git add server/src/poller.rs server/src/db.rs && git commit -m "fix: prevent repeated metube update retries"`


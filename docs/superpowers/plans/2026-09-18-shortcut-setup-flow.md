# iOS Shortcut Setup URL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an authenticated Tubemin user generate a one-time setup URL that the generic iOS Shortcut exchanges for a dedicated API key and saves as `config.json`.

**Architecture:** Add a SQLite-backed, hashed, expiring setup-token table. The authenticated Settings page creates a token and launches the installed Shortcut with the absolute setup URL; the public setup endpoint consumes the token once, creates an API key labeled for the iOS Shortcut, and returns the submit URL plus plaintext key for local Shortcut storage. Preserve the existing `/api/submit` API-key contract.

**Tech Stack:** Rust, Axum, SQLx SQLite migrations, bcrypt, UUID, Minijinja templates, vanilla JavaScript, Docker Compose.

## Global Constraints

- Setup tokens expire after 10 minutes and are single-use.
- Setup tokens are stored hashed, never as plaintext.
- API keys remain revocable through the existing Settings UI.
- Preserve unrelated working-tree changes, especially `docker-compose.yml`.
- Do not modify the signed Shortcut binary in the repository during this backend/frontend change.

---

### Task 1: Add setup-token persistence and service functions

**Files:**
- Create: `server/migrations/<next>_shortcut_setup_tokens.sql`
- Modify: `server/src/api_keys.rs`
- Test: `server/src/api_keys.rs` unit tests

**Interfaces:**
- `create_shortcut_setup_token(pool, owner) -> Result<String, ApiKeyError>` returns plaintext token and stores only a bcrypt hash, owner identity, expiry, and creation time.
- `consume_shortcut_setup_token(pool, plaintext) -> Result<Option<ApiKeyOwnerOwned>, ApiKeyError>` atomically finds a valid unexpired token, deletes it, and returns its owner.

- [ ] Write tests for valid single-use consumption and expired-token rejection.
- [ ] Run the targeted API-key tests and confirm the new tests fail because the functions/table do not exist.
- [ ] Add the SQLite migration with token hash, owner subject/display, created/expiry timestamps, and an index on expiry.
- [ ] Implement bcrypt hashing, UUID token generation, expiry calculation, and a transaction that deletes exactly one matching valid token.
- [ ] Run the targeted tests and confirm they pass.

### Task 2: Add the public setup exchange endpoint

**Files:**
- Create: `server/src/handlers/shortcut.rs`
- Modify: `server/src/handlers/mod.rs`
- Modify: `server/src/main.rs`
- Test: `server/src/handlers/shortcut.rs` tests

**Interfaces:**
- `POST /api/shortcut/setup/:token` consumes the token and returns JSON `{ "submit_url": "<absolute-origin>/api/submit", "api_key": "<new-key>" }`.
- Invalid, expired, or already-consumed tokens return `401` JSON `{ "error": "invalid or expired setup token" }`.

- [ ] Write handler tests for successful exchange and second-use rejection.
- [ ] Run the handler tests and confirm they fail before the route/service exists.
- [ ] Implement token consumption, API-key generation with label `iOS Shortcut`, and absolute-origin construction from forwarded scheme/host headers.
- [ ] Register the route before the existing API routes.
- [ ] Run the targeted handler tests and confirm they pass.

### Task 3: Add authenticated setup-link generation in Settings

**Files:**
- Modify: `server/src/handlers/settings.rs`
- Modify: `server/src/main.rs`
- Modify: `server/templates/settings.html`
- Modify: `server/static/style.css`
- Test: `server/src/handlers/settings.rs` tests

**Interfaces:**
- `POST /settings/shortcut/setup` requires the existing authenticated session and CSRF token, creates a setup token, and redirects back with an encoded setup URL for the modal.
- The modal presents a Configure button that builds `shortcuts://run-shortcut?...` using the current page origin and setup URL; it does not navigate the page to the download endpoint.

- [ ] Add a failing test proving setup-link generation requires CSRF and returns a setup URL for the authenticated owner.
- [ ] Implement the authenticated form handler and route.
- [ ] Replace the current “generate API key for Shortcut” action with “Generate Shortcut Setup”; retain ordinary API-key generation separately.
- [ ] Add client-side URL-scheme construction with `encodeURIComponent`, plus copy and close behavior.
- [ ] Verify the existing blank-page download behavior remains fixed.

### Task 4: Deploy and verify the end-to-end contract

**Files:**
- Modify: `server/static/tubemin-ios-shortcut.shortcut` only if the user supplies a corrected signed Shortcut after backend testing.

- [ ] Run formatting and focused Rust tests.
- [ ] Build and restart only the Tubemin container.
- [ ] Verify `/health`.
- [ ] Verify unauthenticated setup exchange returns `401` rather than creating a key.
- [ ] Verify an authenticated Settings request produces an absolute setup URL.
- [ ] Use the generated URL with the manually updated Shortcut and verify it returns JSON containing `submit_url` and `api_key`.
- [ ] Verify replaying the same setup URL fails.
- [ ] Report any full-suite failures separately from feature failures.

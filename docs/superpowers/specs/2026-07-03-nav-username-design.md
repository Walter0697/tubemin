# Nav Username Display — Login Identity Next to Logout

**Date:** 2026-07-03
**Status:** Approved

## Goal

Show who is logged in, in the topbar nav next to the Logout link, on the
dashboard and settings pages. Password login shows `admin`; OIDC login shows
the Authentik `preferred_username` claim, falling back to the account email
when the claim is absent.

## Approach

Additive optional field on the session user (Approach A) plus extraction of
the duplicated nav markup into a shared minijinja partial.

Rejected alternatives:

- **Replace `email` with a single resolved `display_name` field** — breaks
  deserialization of existing sessions (forced re-login) and discards the
  email.
- **Query the OIDC userinfo endpoint** — extra network round-trip for a claim
  the ID token already carries with the `profile` scope.

## Files touched

| File | Change |
|------|--------|
| `server/src/oidc.rs` | `username` field + `display_name()`, `profile` scope, claim extraction |
| `server/src/password_auth.rs` | set `username: Some("admin")` |
| `server/src/handlers/dashboard.rs` | use the auth user; register nav partial; context vars |
| `server/src/handlers/settings.rs` | same, for the settings GET handler |
| `server/templates/partials/nav.html` | new shared nav partial |
| `server/templates/dashboard.html` | replace inline nav with `{% include "nav" %}` |
| `server/templates/settings.html` | same |
| `server/static/style.css` | `.nav-user` styling + mobile cap |

## Design details

### Session user (`oidc.rs`, `password_auth.rs`)

- `OidcUser` gains `#[serde(default)] pub username: Option<String>` and
  `pub fn display_name(&self) -> &str` returning the username if set, else
  the email. `#[serde(default)]` keeps existing logged-in sessions valid —
  they show the email until the next re-login.
- OIDC login adds `Scope::new("profile".into())` beside the existing email
  scope; the callback extracts
  `claims.preferred_username().map(|u| u.to_string())` into the new field.
- Password login constructs
  `OidcUser { email: "admin".into(), username: Some("admin".into()) }`.

### Shared nav partial (`templates/partials/nav.html`)

The `<nav>…</nav>` block currently duplicated in `dashboard.html` and
`settings.html` moves into one partial, parameterized by:

- `active_page` — `"dashboard"` or `"settings"`; drives which `.nav-link`
  gets the `active` class.
- `username` — rendered as `<span class="nav-user">{{ username }}</span>`
  immediately before the Logout link (after `.nav-spacer`).

Both page templates replace their inline nav with `{% include "nav" %}`.
Each handler's `Environment` registers the partial:
`env.add_template("nav", include_str!("../../templates/partials/nav.html"))`.
The login pages have no nav and are untouched.

### Handlers (`dashboard.rs`, `settings.rs`)

Only the two page-rendering handlers change: `RequireAuth(_user)` →
`RequireAuth(user)`, and the minijinja context gains
`username => user.display_name()` and `active_page`. API handlers
(`submissions.rs`, settings POST actions) keep discarding the user.

### CSS (`style.css`)

- `.nav-user`: `font-size: 12px; color: var(--text-dim);` with
  `overflow: hidden; text-overflow: ellipsis; white-space: nowrap;`.
- In the existing `@media (max-width: 600px)` nav block:
  `.nav-user { max-width: 110px; }` so a long email cannot crowd the nav on
  phones.

## Error handling

No new failure modes: a missing `preferred_username` claim falls back to
email; old sessions deserialize with `username: None`.

## Out of scope

- Unifying the per-handler minijinja `Environment`s.
- Nav on login pages.
- Any change to API authentication.

## Testing

- Existing `cargo test` suite stays green.
- New unit test for `display_name()` (username present → username; absent →
  email).
- Manual: password login shows `admin` in the nav on both pages; OIDC path
  verified by code review plus the user's next real login (headless OIDC not
  testable locally). Existing-session fallback: current session shows email
  until re-login.

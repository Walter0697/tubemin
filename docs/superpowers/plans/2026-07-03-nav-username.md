# Nav Username Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the logged-in identity (password login → `admin`, OIDC → `preferred_username` falling back to email) next to the Logout link, via a shared nav partial.

**Architecture:** `OidcUser` (the session user stored by both auth flows) gains an optional `username` field and a `display_name()` accessor. The duplicated `<nav>` markup in dashboard.html/settings.html moves into a shared minijinja partial parameterized by `active_page` and `username`; the two page handlers pass those into the render context. One small CSS rule styles the name.

**Tech Stack:** Rust (axum, openidconnect 3, minijinja 2, serde), server-rendered templates.

**Spec:** `docs/superpowers/specs/2026-07-03-nav-username-design.md`

## Global Constraints

- Existing sessions must keep deserializing: the new `OidcUser.username` field carries `#[serde(default)]`.
- Only the two page-rendering handlers (`dashboard`, `settings` GET) consume the auth user; API handlers (`submissions.rs`, settings POST actions) keep the `RequireAuth(_user)` discard pattern.
- Login pages (`login.html`, `login_oidc.html`) are untouched — they have no nav.
- Display precedence: `username` if `Some`, else `email`. Password login sets both to `"admin"`. OIDC sets `username` from the `preferred_username` ID-token claim (requires adding the `profile` scope).
- All work happens on branch `feat/mobile-responsive` in `/Users/walter/Documents/git/tubemin`.
- `cargo test` (31 existing tests) must stay green after every task.

---

### Task 1: Session user identity — `username` field, `display_name()`, both auth flows

**Files:**
- Modify: `server/src/oidc.rs:23-26` (struct), `:78-81` (scopes), `:146-154` (callback claims), end of file (tests)
- Modify: `server/src/password_auth.rs:30-33`

**Interfaces:**
- Produces: `OidcUser { email: String, username: Option<String> }` and `pub fn display_name(&self) -> &str` — Task 2's handlers call `user.display_name()`.

- [ ] **Step 1: Write the failing tests**

Append at the end of `server/src/oidc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_prefers_username() {
        let u = OidcUser { email: "a@b.c".into(), username: Some("walter".into()) };
        assert_eq!(u.display_name(), "walter");
    }

    #[test]
    fn display_name_falls_back_to_email() {
        let u = OidcUser { email: "a@b.c".into(), username: None };
        assert_eq!(u.display_name(), "a@b.c");
    }

    #[test]
    fn old_session_json_deserializes_with_default_username() {
        let u: OidcUser = serde_json::from_str(r#"{"email":"a@b.c"}"#).unwrap();
        assert!(u.username.is_none());
        assert_eq!(u.display_name(), "a@b.c");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd server && cargo test oidc:: 2>&1 | tail -5`
Expected: compile error — `OidcUser` has no field `username`, no method `display_name`.

- [ ] **Step 3: Implement the struct change and accessor**

In `server/src/oidc.rs`, replace the `OidcUser` struct (lines 23-26) with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUser {
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
}

impl OidcUser {
    /// Name shown in the UI: OIDC preferred_username when present, else email.
    pub fn display_name(&self) -> &str {
        self.username.as_deref().unwrap_or(&self.email)
    }
}
```

- [ ] **Step 4: Wire the OIDC flow**

In `server/src/oidc.rs` `login()`, add the `profile` scope after the existing `email` scope (line 79):

```rust
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
```

In `callback()`, after the `email` extraction (lines 146-149), add the username extraction and include it in the session insert (replace lines 146-154):

```rust
            let email = claims
                .email()
                .map(|e| e.as_str().to_string())
                .unwrap_or_else(|| "unknown".into());

            let username = claims
                .preferred_username()
                .map(|u| u.as_str().to_string());

            session
                .insert(SESSION_USER_KEY, OidcUser { email, username })
                .await
                .ok();
```

- [ ] **Step 5: Wire the password flow**

In `server/src/password_auth.rs` (line 31), replace the session insert:

```rust
        session
            .insert(
                SESSION_USER_KEY,
                OidcUser { email: "admin".into(), username: Some("admin".into()) },
            )
            .await
            .ok();
```

- [ ] **Step 6: Run the full test suite**

Run: `cd server && cargo test 2>&1 | tail -3`
Expected: `test result: ok. 34 passed; 0 failed` (31 existing + 3 new). No new warnings.

- [ ] **Step 7: Commit**

```bash
git add server/src/oidc.rs server/src/password_auth.rs
git commit -m "feat: capture login username (OIDC preferred_username / admin) in session"
```

---

### Task 2: Shared nav partial, template includes, handler context

**Files:**
- Create: `server/templates/partials/nav.html`
- Modify: `server/templates/dashboard.html:11-17` (nav block)
- Modify: `server/templates/settings.html:11-17` (nav block)
- Modify: `server/src/handlers/dashboard.rs` (env registration, handler signature, context, render test)
- Modify: `server/src/handlers/settings.rs:26,43-57` (env registration, handler signature, context)

**Interfaces:**
- Consumes: `OidcUser::display_name(&self) -> &str` from Task 1.
- Produces: minijinja template name `"nav"` expecting context vars `active_page` (`"dashboard"` | `"settings"`) and `username` (string).

- [ ] **Step 1: Create the partial**

Create `server/templates/partials/nav.html` — the exact nav markup currently duplicated in both pages, with the `active` class made conditional and the username span added:

```html
<nav>
  <span class="nav-logo"><img src="/static/favicon.png" class="nav-icon" alt="">TUBEMIN</span>
  <a href="/dashboard" class="nav-link{% if active_page == 'dashboard' %} active{% endif %}">Dashboard</a>
  <a href="/settings" class="nav-link{% if active_page == 'settings' %} active{% endif %}">Settings</a>
  <span class="nav-spacer"></span>
  <span class="nav-user">{{ username }}</span>
  <a href="/auth/logout" class="nav-link nav-logout">Logout</a>
</nav>
```

- [ ] **Step 2: Replace both inline navs with the include**

In `server/templates/dashboard.html`, replace lines 11-17 (`<nav>` through `</nav>`) with:

```html
  {% include "nav" %}
```

Do the same for `server/templates/settings.html` lines 11-17.

- [ ] **Step 3: Write the failing render test**

Append at the end of `server/src/handlers/dashboard.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_renders_nav_with_username_and_active_tab() {
        let tmpl = dashboard_env().get_template("dashboard").unwrap();
        let html = tmpl
            .render(minijinja::context! {
                peertube_base => "",
                username => "admin",
                active_page => "dashboard",
            })
            .unwrap();
        assert!(html.contains(r#"<span class="nav-user">admin</span>"#));
        assert!(html.contains(r#"class="nav-link active""#));
    }
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cd server && cargo test dashboard_renders 2>&1 | tail -5`
Expected: FAIL — the render errors with `template not found: nav` (the include exists but the env doesn't know the partial yet), surfacing as `.unwrap()` panic.

- [ ] **Step 5: Register the partial and pass the context — dashboard**

In `server/src/handlers/dashboard.rs`:

In `dashboard_env()` (after line 10, before adding the dashboard template):

```rust
        env.add_template("nav", include_str!("../../templates/partials/nav.html")).unwrap();
```

In the handler, change the extractor (line 17) from `RequireAuth(_user): RequireAuth` to:

```rust
    RequireAuth(user): RequireAuth,
```

And add to the `minijinja::context!` block (after `peertube_base => peertube_base,`):

```rust
        username => user.display_name(),
        active_page => "dashboard",
```

- [ ] **Step 6: Register the partial and pass the context — settings**

In `server/src/handlers/settings.rs`, in the `settings` GET handler only:

Change the extractor (line 26) from `RequireAuth(_user): RequireAuth` to `RequireAuth(user): RequireAuth`.

After `env.set_auto_escape_callback(...)` (line 44), add:

```rust
    env.add_template("nav", include_str!("../../templates/partials/nav.html")).unwrap();
```

Add to the `minijinja::context!` block (before `new_key => query.new_key,`):

```rust
        username => user.display_name(),
        active_page => "settings",
```

Leave `generate_key` and `revoke_key` untouched (`RequireAuth(_user)` stays).

- [ ] **Step 7: Run the full test suite**

Run: `cd server && cargo test 2>&1 | tail -3`
Expected: `test result: ok. 35 passed; 0 failed` (34 from Task 1 + 1 render test). No new warnings.

- [ ] **Step 8: Commit**

```bash
git add server/templates/partials/nav.html server/templates/dashboard.html server/templates/settings.html server/src/handlers/dashboard.rs server/src/handlers/settings.rs
git commit -m "feat: shared nav partial showing logged-in username"
```

---

### Task 3: Nav username CSS

**Files:**
- Modify: `server/static/style.css` (nav section ~line 84-85, and the first `@media (max-width: 600px)` block ~line 783)

**Interfaces:**
- Consumes: `.nav-user` class emitted by the Task 2 partial.

- [ ] **Step 1: Add the base rule**

In `server/static/style.css`, in the Topbar nav section, after the `.nav-logout:hover` rule (line 85 `.nav-logout:hover { color: var(--text); }`), add:

```css
.nav-user {
  font-size: 12px;
  color: var(--text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

(This is intentionally outside media queries: the username is a new visible element on all screen sizes.)

- [ ] **Step 2: Cap its width on phones**

In the first `@media (max-width: 600px)` block of the Responsive section (the one containing the `nav` and `.nav-logo` rules, ~line 783), add after the `.nav-logo { margin-right: 0; }` rule:

```css
  .nav-user { max-width: 110px; }
```

- [ ] **Step 3: Sanity check**

Run: `grep -c "nav-user" server/static/style.css server/templates/partials/nav.html`
Expected: `server/static/style.css:2`, `server/templates/partials/nav.html:1`.

Run: `python3 -c "css=open('server/static/style.css').read(); print(css.count('{')==css.count('}'))"`
Expected: `True`

- [ ] **Step 4: Commit**

```bash
git add server/static/style.css
git commit -m "feat: nav username styling with mobile width cap"
```

---

### Task 4: Rebuild and verification

**Files:**
- None modified — verification only.

**Interfaces:**
- Consumes: everything from Tasks 1-3.

- [ ] **Step 1: Rebuild and restart the local stack**

Templates/static are baked into the image (use the `devops-docker-rebuild` skill if executing in Claude Code; otherwise):

```bash
cd /Users/walter/Documents/git/tubemin
docker compose -f docker-compose.yml -f docker-compose.local.yml build tubemin && \
docker compose -f docker-compose.yml -f docker-compose.local.yml up -d tubemin
```

Expected: rebuild succeeds; app reachable at `http://localhost:3000`.

- [ ] **Step 2: Automated served-artifact checks**

```bash
curl -s http://localhost:3000/static/style.css | grep -c "nav-user"        # expect 2
docker exec tubemin-tubemin-1 grep -c "nav-user" templates/partials/nav.html  # expect 1
docker exec tubemin-tubemin-1 grep -c 'include "nav"' templates/dashboard.html templates/settings.html  # expect 1 each
```

- [ ] **Step 3: Manual verification (user, or note as pending)**

The local instance is behind OIDC, so the rendered nav needs an authenticated browser:
- Log in via OIDC → nav shows the Authentik `preferred_username` (an existing session shows the email until re-login).
- On a password-auth deployment → nav shows `admin`.
- On a phone width, a long name truncates with an ellipsis instead of crowding the nav.

If verification surfaces fixes, commit them as `fix: nav username adjustments from verification`.

# Tubemin UI Reskin — Design Spec

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply a consistent "Dark Media" visual theme across all five HTML surfaces in the project — three server-rendered dashboard pages and two Chrome extension pages.

**Architecture:** Plain CSS with CSS custom properties (variables) for the color palette. Server pages share one `style.css` served as a static file. Extension pages share one `theme.css` bundled in the extension. No build step, no framework.

**Tech Stack:** Plain CSS, Axum `ServeDir` for static assets, minijinja templates, Chrome extension Manifest V3.

---

## Global Constraints

- No external CDN dependencies — everything must work offline / self-hosted.
- No JS frameworks or CSS frameworks — plain CSS only.
- No changes to routing, template data, or Rust business logic — this is a pure front-end reskin.
- All existing template variables (`{{ s.url }}`, `{{ csrf_token }}`, etc.) stay unchanged.
- The extension must continue to work with Chrome's Content Security Policy — no inline `style` attributes on dynamic content, no `eval`.
- Maintain all existing interactive states: button disabled, hint messages, status classes, new-key banner, revoke form.

---

## Design Tokens

Defined as CSS custom properties on `:root` in both `style.css` and `theme.css`:

```css
:root {
  --bg:           #0f0f0f;   /* page background */
  --surface:      #1a1a1a;   /* cards, topbar, input bg */
  --surface-2:    #242424;   /* elevated surface, disabled button */
  --border:       #2e2e2e;   /* dividers and input borders */
  --accent:       #e85d4a;   /* coral — primary action color */
  --accent-hover: #d44a37;   /* accent on hover */
  --text:         #e0e0e0;   /* primary text */
  --text-muted:   #888888;   /* secondary text, nav items */
  --text-dim:     #555555;   /* table headers, placeholders */
  --status-ok:    #4ade80;   /* imported / success */
  --status-err:   #e85d4a;   /* error (same as accent) */
  --mono: ui-monospace, 'JetBrains Mono', 'Fira Mono', monospace;
}
```

---

## Shared Components

These styles appear in both `style.css` (server) and `theme.css` (extension).

### Typography
- Body font: `system-ui, -apple-system, sans-serif`
- Monospace (URLs, key IDs): `var(--mono)`
- Brand mark: `▶ TUBEMIN` — `var(--accent)`, `font-weight: 800`, `letter-spacing: 2px`, `font-size: 12px`

### Buttons
```
Primary (enabled):
  background: var(--accent)
  color: white
  border: none
  border-radius: 6px
  padding: 9px 16px
  font-size: 11px
  font-weight: 700
  letter-spacing: 1px
  text-transform: uppercase
  cursor: pointer

  :hover → background: var(--accent-hover)

Disabled:
  background: var(--surface-2)
  color: var(--text-dim)
  cursor: not-allowed

Secondary (e.g. Revoke, Change):
  background: var(--surface-2)
  color: var(--text-muted)
  border: 1px solid var(--border)
  :hover → border-color: var(--text-dim), color: var(--text)
```

### Inputs
```
background: var(--surface)
border: 1px solid var(--border)
border-radius: 6px
padding: 8px 10px
color: var(--text)
font-size: 0.9rem
width: 100%; box-sizing: border-box

:focus → outline: none; border-color: var(--accent)
::placeholder → color: var(--text-dim)
```

### Status dots (dashboard table)
```html
<span class="status-dot status-imported"></span>
<span class="status-dot status-pending"></span>
<span class="status-dot status-error"></span>
```
```css
.status-dot {
  display: inline-block;
  width: 6px; height: 6px;
  border-radius: 50%;
  margin-right: 6px;
  vertical-align: middle;
}
.status-imported { background: var(--status-ok);  box-shadow: 0 0 6px #4ade8066; }
.status-pending  { background: var(--text-dim); }
.status-error    { background: var(--status-err); box-shadow: 0 0 6px #e85d4a66; }
```

---

## Server: `style.css`

File location: `server/static/style.css`
Served at: `/static/style.css` via `ServeDir` added to the Axum router.
All three server templates (`dashboard.html`, `settings.html`, `login.html`) link to it with:
```html
<link rel="stylesheet" href="/static/style.css">
```
All existing inline `<style>` blocks are removed from the templates.

### Base / layout
```css
*, *::before, *::after { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: system-ui, -apple-system, sans-serif;
  min-height: 100vh;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
```

### Topbar (`<nav>`)
```html
<nav>
  <span class="nav-logo">▶ TUBEMIN</span>
  <a href="/dashboard" class="nav-link [active]">Dashboard</a>
  <a href="/settings"  class="nav-link">Settings</a>
</nav>
```
```css
nav {
  display: flex;
  align-items: center;
  gap: 24px;
  padding: 10px 24px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  position: sticky; top: 0;
}
.nav-logo {
  color: var(--accent);
  font-size: 12px;
  font-weight: 800;
  letter-spacing: 2px;
  margin-right: 8px;
}
.nav-link {
  color: var(--text-muted);
  font-size: 13px;
  text-decoration: none;
  padding-bottom: 2px;
}
.nav-link:hover { color: var(--text); }
.nav-link.active {
  color: var(--text);
  border-bottom: 2px solid var(--accent);
}
```

### Page content wrapper
```css
.page { max-width: 900px; margin: 32px auto; padding: 0 24px; }
.page-title {
  font-size: 1.25rem;
  font-weight: 600;
  margin: 0 0 24px;
  color: var(--text);
}
.section-title {
  font-size: 10px;
  letter-spacing: 2px;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 12px;
}
```

### Table (dashboard + settings)
```css
.data-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
.data-table th {
  text-align: left;
  padding: 6px 12px;
  font-size: 10px;
  color: var(--text-dim);
  letter-spacing: 1px;
  text-transform: uppercase;
  font-weight: 600;
  border-bottom: 1px solid var(--border);
}
.data-table td {
  padding: 10px 12px;
  color: var(--text-muted);
  border-bottom: 1px solid #1e1e1e;
}
.data-table td:first-child { color: var(--text); }
.data-table tr:last-child td { border-bottom: none; }
.data-table td a { color: var(--text); }
```

### New key banner (settings)
```css
.new-key {
  background: var(--surface);
  border-left: 3px solid var(--accent);
  border-radius: 0 6px 6px 0;
  padding: 12px 16px;
  margin-bottom: 20px;
  font-size: 13px;
  color: var(--text-muted);
}
.new-key strong { color: var(--text); }
.new-key code {
  display: block;
  margin-top: 6px;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--accent);
  word-break: break-all;
}
```

### Login page
```css
.login-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
}
.login-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 32px;
  width: 340px;
}
.login-logo {
  color: var(--accent);
  font-size: 14px;
  font-weight: 800;
  letter-spacing: 2px;
  margin-bottom: 24px;
}
.login-error {
  background: #2a1212;
  border-left: 3px solid var(--accent);
  border-radius: 0 4px 4px 0;
  padding: 8px 12px;
  font-size: 12px;
  color: #ffaaaa;
  margin-bottom: 16px;
}
label {
  display: block;
  font-size: 11px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--text-dim);
  margin-bottom: 6px;
}
```

---

## Extension: `theme.css`

File location: `extension/theme.css`
Both `popup.html` and `settings.html` link to it with:
```html
<link rel="stylesheet" href="theme.css">
```
All existing inline `<style>` blocks are removed from both extension HTML files.

### Base
```css
*, *::before, *::after { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: system-ui, -apple-system, sans-serif;
}
a { color: var(--accent); text-decoration: underline; cursor: pointer; }
```

### Popup layout (`popup.html` — 260px wide)
```css
.popup-body { width: 260px; }

.popup-topbar {
  display: flex;
  align-items: center;
  padding: 10px 14px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
}
.popup-logo {
  color: var(--accent);
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 2px;
}
.popup-settings-link {
  margin-left: auto;
  color: var(--text-dim);
  font-size: 14px;
  text-decoration: none;
  cursor: pointer;
}
.popup-settings-link:hover { color: var(--text-muted); }

.popup-content { padding: 14px; }

.url-box {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 10px;
  margin-bottom: 12px;
}
.url-box-site {
  font-size: 9px;
  color: var(--text-dim);
  letter-spacing: 1px;
  text-transform: uppercase;
  margin-bottom: 3px;
}
.url-box-text {
  font-size: 10px;
  color: var(--text);
  font-family: var(--mono);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

#hint {
  font-size: 10px;
  color: var(--text-dim);
  margin-top: 8px;
  min-height: 1em;
  text-align: center;
}
#hint.warn { color: #e8884a; }

#status {
  font-size: 11px;
  margin-top: 8px;
  min-height: 1.2em;
  text-align: center;
}
#status.success { color: var(--status-ok); }
#status.error   { color: var(--status-err); }

.popup-footer {
  padding: 8px 14px;
  border-top: 1px solid var(--border);
  font-size: 10px;
  color: var(--text-dim);
  text-align: right;
}
```

### Extension settings layout (`settings.html` — centered, 480px max)
```css
.settings-body {
  max-width: 480px;
  margin: 32px auto;
  padding: 0 20px;
}
.settings-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--accent);
  letter-spacing: 2px;
  text-transform: uppercase;
  margin-bottom: 24px;
}
.field-label {
  display: block;
  font-size: 10px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--text-dim);
  margin-bottom: 6px;
}
.field-row {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 16px;
}
.field-row input { margin-bottom: 0; flex: 1; }
.key-masked {
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text-muted);
}
#status.ok  { color: var(--status-ok); }
#status.err { color: var(--status-err); }
```

---

## Template Changes

### `server/templates/dashboard.html`
- Remove inline `<style>` block
- Add `<link rel="stylesheet" href="/static/style.css">`
- Wrap body content in `<div class="page">`
- Add `class="nav-logo"` / `class="nav-link active"` / `class="nav-link"` to `<nav>`
- Add `class="page-title"` to `<h1>`
- Add `class="section-title"` above table
- Add `class="data-table"` to `<table>`
- Replace `class="status-pending/imported/error"` on `<td>` with `<span class="status-dot status-pending/imported/error"></span>` inside the status cell

### `server/templates/settings.html`
- Remove inline `<style>` block
- Add `<link rel="stylesheet" href="/static/style.css">`
- Same nav changes as dashboard
- Wrap body in `<div class="page">`
- Add `class="new-key"` already exists — keep it, styles change
- Add `class="data-table"` to `<table>`
- Add `class="section-title"` to `<h2>`

### `server/templates/login.html`
- Remove inline `<style>` block
- Add `<link rel="stylesheet" href="/static/style.css">`
- Wrap entire body in `<div class="login-wrap"><div class="login-card">`
- Add `class="login-logo"` to the brand heading
- Replace `<!--ERROR-->` error injection with `<div class="login-error">` wrapper

### `extension/popup.html`
- Remove inline `<style>` block
- Add `<link rel="stylesheet" href="theme.css">`
- Wrap body in `<div class="popup-body">`
- Add topbar `<div class="popup-topbar">` with logo + settings gear link
- Wrap URL preview in `<div class="url-box"><div class="url-box-site"></div><div class="url-box-text"></div></div>`
- Move settings link from `<div id="footer">` to topbar gear icon
- `popup.js`: when hint is a warning ("This site isn't supported"), add class `warn` to `#hint`

### `extension/settings.html`
- Remove inline `<style>` block
- Add `<link rel="stylesheet" href="theme.css">`
- Wrap body in `<div class="settings-body">`
- Apply `class="field-label"` to all `<label>` elements
- Apply `class="key-masked"` to `#key-masked` span

---

## Server Static File Serving

Add `tower_http::services::ServeDir` to the Axum router in `main.rs`:

```rust
use tower_http::services::ServeDir;

// In the router:
.nest_service("/static", ServeDir::new("static"))
```

Create directory `server/static/` and place `style.css` there.
The `ServeDir` path is relative to the working directory when the server runs — inside Docker this is `/app`, so the Dockerfile must copy `server/static/` into `/app/static/`.

---

## Out of Scope

- Dark mode toggle or light mode variant
- Animations beyond `:hover` transitions
- Responsive / mobile layout for the dashboard
- Font loading from Google Fonts or Bunny Fonts

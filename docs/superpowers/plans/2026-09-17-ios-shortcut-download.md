# iOS Shortcut Download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the uploaded signed Tubemin iOS Shortcut downloadable from the authenticated Settings page.

**Architecture:** Store the signed Shortcut unchanged as a static server asset. Add an authenticated Settings-page download link that points to that asset; this deliberately avoids modifying the signed file before the iOS import behavior is confirmed.

**Tech Stack:** Rust/Axum, Minijinja templates, static asset serving, Apple `.shortcut` file.

## Global Constraints

- The Shortcut is hosted by Tubemin as a downloadable file; no iCloud Shortcut link is used.
- The signed Shortcut file must be served unchanged.
- The download control is available only from the authenticated Settings page.
- Do not expose or generate an API key as part of this first download-only test.

---

### Task 1: Add the signed Shortcut asset

**Files:**
- Create: `server/static/tubemin-ios-shortcut.shortcut`
- Source: `Send To Tubemin.shortcut`

**Interfaces:**
- Produces: a static `/static/tubemin-ios-shortcut.shortcut` resource served by the existing `ServeDir` route.

- [ ] **Step 1: Copy the uploaded signed file without transformation**

Run:

```bash
cp "Send To Tubemin.shortcut" server/static/tubemin-ios-shortcut.shortcut
```

- [ ] **Step 2: Verify the copied asset is byte-identical**

Run:

```bash
cmp "Send To Tubemin.shortcut" server/static/tubemin-ios-shortcut.shortcut
```

Expected: no output and exit code 0.

- [ ] **Step 3: Commit the asset**

```bash
git add server/static/tubemin-ios-shortcut.shortcut
git commit -m "feat: add iOS shortcut download asset"
```

### Task 2: Add the Settings download control

**Files:**
- Modify: `server/templates/settings.html`
- Modify: `server/static/style.css`

**Interfaces:**
- Consumes: the static asset from Task 1 at `/static/tubemin-ios-shortcut.shortcut`.
- Produces: a visible Settings-page link labeled `Download iOS Shortcut`.

- [ ] **Step 1: Add the download link beside the API-key controls**

Add a normal anchor in the API Keys section:

```html
<a class="btn-secondary" href="/static/tubemin-ios-shortcut.shortcut" download>
  Download iOS Shortcut
</a>
<p class="form-help">Install the Shortcut, then configure its Tubemin server URL and API key.</p>
```

Keep the link on the authenticated Settings page; do not add a new public API endpoint.

- [ ] **Step 2: Verify the template renders the expected link**

Run:

```bash
rtk rg -n "Download iOS Shortcut|tubemin-ios-shortcut" server/templates/settings.html
```

Expected: both strings appear once.

- [ ] **Step 3: Commit the Settings UI**

```bash
git add server/templates/settings.html server/static/style.css
git commit -m "feat: add iOS shortcut download to settings"
```

### Task 3: Verify the complete download path

**Files:**
- Test: `server/static/tubemin-ios-shortcut.shortcut`
- Test: `server/templates/settings.html`

- [ ] **Step 1: Run formatting and the Rust test suite**

Run:

```bash
rtk cargo fmt -- --check
rtk cargo test
```

Expected: formatting passes and all tests pass.

- [ ] **Step 2: Verify the working tree and asset integrity**

Run:

```bash
git status --short
cmp "Send To Tubemin.shortcut" server/static/tubemin-ios-shortcut.shortcut
```

Expected: only the intended user-provided Shortcut metadata may remain untracked; the copied asset is tracked and byte-identical.

- [ ] **Step 3: Manually verify on iPhone**

Open Settings in Safari, tap `Download iOS Shortcut`, open the downloaded file, and confirm iOS offers `Add Shortcut`. Then test the Shortcut from YouTube Share Sheet.

# Mobile Responsiveness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the tubemin web UI (login, login_oidc, dashboard, settings) usable on phones and small tablets via additive responsive CSS plus touch-device select-mode support.

**Architecture:** Desktop styles stay the default. A single "Responsive" section is appended to `server/static/style.css` containing `max-width: 900px` / `max-width: 600px` breakpoints and one `@media (hover: none)` block. One structural HTML change: the settings table gets an `overflow-x` scroll wrapper. No JS-logic or backend changes.

**Tech Stack:** Plain CSS (media queries), minijinja HTML templates, Rust/axum server (untouched), Docker for verification.

**Spec:** `docs/superpowers/specs/2026-07-03-mobile-responsive-design.md`

## Global Constraints

- Desktop layout (>900px viewport) must be pixel-identical to today — all responsive rules live inside media queries.
- No JavaScript changes. Touch select-mode must work with the existing JS (checkbox click already enters select mode).
- No backend/API changes. Only `server/static/style.css` and `server/templates/settings.html` are modified.
- Browser-extension pages (`extension/`) are out of scope.
- CSS is appended as one clearly-labeled section at the end of `style.css`, matching the file's existing `/* ── Section ── */` comment style.
- Templates and static files are compiled into the Docker image; visual verification requires a rebuild (Task 6). Tasks 1–5 verify by CSS/HTML inspection and commit; full visual verification happens once in Task 6.

---

### Task 1: Settings table scroll wrapper

**Files:**
- Modify: `server/templates/settings.html` (around lines 36–66, the `.data-table`)
- Modify: `server/static/style.css` (append)

**Interfaces:**
- Produces: `.table-scroll` class used only by settings.html.

- [ ] **Step 1: Wrap the table in settings.html**

In `server/templates/settings.html`, wrap the existing `<table class="data-table">…</table>` element (the whole table, including `{% for %}` rows) in a scroll container:

```html
    <div class="table-scroll">
      <table class="data-table">
        <!-- existing thead/tbody unchanged -->
      </table>
    </div>
```

Only add the opening `<div class="table-scroll">` before `<table class="data-table">` and the closing `</div>` after `</table>`. Do not change anything inside the table.

- [ ] **Step 2: Add the scroll-wrapper CSS**

Append to the end of `server/static/style.css`:

```css
/* ── Responsive ─────────────────────────────────── */
.table-scroll {
  overflow-x: auto;
  -webkit-overflow-scrolling: touch;
  margin-bottom: 24px;
}
.table-scroll .data-table {
  min-width: 560px;
  margin-bottom: 0;
}
```

(The `margin-bottom` moves from the table to the wrapper so spacing is unchanged.)

- [ ] **Step 3: Sanity check**

Run: `grep -c "table-scroll" server/templates/settings.html server/static/style.css`
Expected: `server/templates/settings.html:1` (the div appears once as an opening tag — grep counts lines, opening and closing tags are on different lines so this reports 2 lines; accept `2`) and `server/static/style.css:2`.

Visually inspect the diff: `git diff` — confirm the table markup itself is unchanged, only wrapped.

- [ ] **Step 4: Commit**

```bash
git add server/templates/settings.html server/static/style.css
git commit -m "feat: horizontal scroll wrapper for settings API-keys table"
```

---

### Task 2: Grid, page, nav, and login breakpoints

**Files:**
- Modify: `server/static/style.css` (append inside the `/* ── Responsive ── */` section created in Task 1)

**Interfaces:**
- Consumes: existing classes `.card-grid`, `.page`, `nav`, `.nav-logo`, `.login-card`.
- Produces: nothing new — media-query overrides only.

- [ ] **Step 1: Append breakpoint CSS**

Append to `server/static/style.css` (after the Task 1 rules):

```css
/* Tablet: 2-column card grid */
@media (max-width: 900px) {
  .card-grid {
    grid-template-columns: repeat(2, 1fr);
  }
}

/* Phone */
@media (max-width: 600px) {
  .card-grid {
    grid-template-columns: 1fr;
  }
  .page {
    margin: 20px auto;
    padding: 0 16px;
  }
  nav {
    gap: 14px;
    padding: 10px 16px;
  }
  .nav-logo { margin-right: 0; }
  .login-card {
    width: 100%;
    max-width: 340px;
    margin: 0 16px;
  }
}
```

- [ ] **Step 2: Sanity check**

Run: `grep -c "max-width: 900px\|max-width: 600px" server/static/style.css`
Expected: `2`

Confirm all new rules are inside `@media` blocks (desktop untouched): review `git diff server/static/style.css`.

- [ ] **Step 3: Commit**

```bash
git add server/static/style.css
git commit -m "feat: responsive breakpoints for card grid, page, nav, login"
```

---

### Task 3: Swipeable filter tabs on phone

**Files:**
- Modify: `server/static/style.css` (append inside the Responsive section)

**Interfaces:**
- Consumes: existing `.filter-tabs`, `.filter-tab` (dashboard.html — no HTML change).

- [ ] **Step 1: Append filter-tab CSS**

Append to `server/static/style.css`:

```css
/* Phone: filter tabs — one swipeable row (YouTube-chips style) */
@media (max-width: 600px) {
  .filter-tabs {
    flex-wrap: nowrap;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    scrollbar-width: none;          /* Firefox */
    margin: 0 -16px;                /* bleed to page edges */
    padding: 0 16px 4px;
  }
  .filter-tabs::-webkit-scrollbar { display: none; }
  .filter-tab {
    flex-shrink: 0;
    white-space: nowrap;
  }
}
```

The `-16px` bleed must match the `.page` side padding set in Task 2 (16px). If Task 2's padding value changes, change this too.

- [ ] **Step 2: Sanity check**

Run: `grep -n "filter-tabs" server/static/style.css | tail -3`
Expected: new rules present at the end of the file, inside a `@media (max-width: 600px)` block.

- [ ] **Step 3: Commit**

```bash
git add server/static/style.css
git commit -m "feat: horizontally swipeable dashboard filter tabs on phone"
```

---

### Task 4: Touch-device select mode, bulk toolbar, tap targets

**Files:**
- Modify: `server/static/style.css` (append inside the Responsive section)

**Interfaces:**
- Consumes: existing `.card-checkbox`, `.bulk-toolbar`, `.filter-tab`, `.pagination button`. Existing JS in dashboard.html already enters select mode when a checkbox is clicked — no JS change.

- [ ] **Step 1: Append touch CSS**

Append to `server/static/style.css`:

```css
/* Touch devices: no hover — checkboxes must be reachable */
@media (hover: none) {
  .card-checkbox { display: block; }   /* always visible → select mode reachable */
  /* quick-delete stays hidden; touch deletes via select mode or detail modal */
  .bulk-toolbar { flex-wrap: wrap; }
  .filter-tab { min-height: 40px; }
  .pagination button { min-height: 40px; }
}
```

- [ ] **Step 2: Sanity check**

Run: `grep -n "hover: none" server/static/style.css`
Expected: exactly one match, in the newly appended block.

Confirm no rule outside `@media` blocks was touched: `git diff server/static/style.css`.

- [ ] **Step 3: Commit**

```bash
git add server/static/style.css
git commit -m "feat: touch-device select mode and tap-target sizing"
```

---

### Task 5: Modal scrolling on small screens

**Files:**
- Modify: `server/static/style.css` (append inside the Responsive section)

**Interfaces:**
- Consumes: existing `.modal-box`, `.detail-modal-body`.

- [ ] **Step 1: Append modal CSS**

Append to `server/static/style.css`:

```css
/* Phone: modals scroll instead of overflowing the viewport */
@media (max-width: 600px) {
  .modal-box {
    max-height: 90dvh;
    overflow-y: auto;
  }
  .detail-modal-body {
    padding: 14px 16px 16px;
  }
}
```

- [ ] **Step 2: Sanity check**

Run: `grep -n "90dvh" server/static/style.css`
Expected: one match.

- [ ] **Step 3: Commit**

```bash
git add server/static/style.css
git commit -m "feat: scrollable modals on small screens"
```

---

### Task 6: Rebuild and visual verification

**Files:**
- None modified — verification only.

**Interfaces:**
- Consumes: everything from Tasks 1–5.

- [ ] **Step 1: Rebuild and start the local stack**

Templates/static are baked into the image, so rebuild (use the `devops-docker-rebuild` skill if executing in Claude Code; otherwise):

```bash
cd /Users/walter/Documents/git/tubemin
docker compose -f docker-compose.yml -f docker-compose.local.yml up --build -d tubemin
```

Expected: tubemin container rebuilds and starts; app reachable at `http://localhost:3000`.

- [ ] **Step 2: Verify each page at each width**

In a browser (devtools responsive mode) check `http://localhost:3000` pages at 360px, 600px, 900px, and full desktop width:

| Page | 360px expectation | Desktop expectation |
|------|-------------------|---------------------|
| /login | Card fills width minus 16px margins, no horizontal scroll | Unchanged 340px centered card |
| /dashboard | 1-column cards; filter tabs swipe horizontally in one row; no page horizontal scroll | Unchanged 3-column grid, tabs in one static row |
| /dashboard (900px) | 2-column cards | — |
| /settings | Table swipes sideways inside its wrapper; page itself doesn't scroll horizontally | Unchanged table |

- [ ] **Step 3: Verify touch behavior**

In devtools, enable touch emulation (device toolbar), reload /dashboard:
- Card checkboxes are visible without hover.
- Tapping a checkbox enters select mode and shows the bulk toolbar; toolbar buttons wrap if needed.
- Tapping a card (not the checkbox) opens the detail modal; modal scrolls if taller than the viewport; Delete works from the modal.

- [ ] **Step 4: Desktop regression check**

At >900px with touch emulation OFF: hover a card — checkbox and quick-delete button appear on hover exactly as before; grid is 3 columns; nav spacing unchanged.

- [ ] **Step 5: Fix anything found, then final commit if fixes were made**

If verification surfaced fixes:

```bash
git add server/static/style.css server/templates/settings.html
git commit -m "fix: mobile responsiveness adjustments from visual verification"
```

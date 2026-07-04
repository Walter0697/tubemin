# Mobile Responsiveness — Login, Dashboard, Settings

**Date:** 2026-07-03
**Status:** Approved

## Goal

Make the tubemin web UI (login, login_oidc, dashboard, settings pages) usable on
mobile phones and small tablets. Today the stylesheet has no media queries: the
dashboard renders a fixed 3-column grid, the 7 filter tabs overflow
horizontally, the settings table overflows, and — because card checkboxes and
quick-delete buttons only appear on `:hover` — touch devices have no way to
enter select mode or bulk-delete.

## Approach

Additive, desktop-default responsive CSS (Approach A). Desktop styles stay the
base; a responsive section appended to `server/static/style.css` layers in
`max-width` breakpoints and `@media (hover: none)` touch handling. No JS-logic
changes; one structural HTML change (settings table scroll wrapper).

Rejected alternatives:

- **Mobile-first stylesheet rewrite** — touches every rule, high desktop
  regression risk, small payoff for a ~760-line stylesheet.
- **CSS framework (Tailwind etc.)** — overkill for four templates; fights the
  existing hand-rolled design tokens.

## Files touched

| File | Change |
|------|--------|
| `server/static/style.css` | Responsive section appended (bulk of work) |
| `server/templates/settings.html` | Wrap `.data-table` in `.table-scroll` div |

## Design details

### Breakpoints

- **≤900px**: `.card-grid` → 2 columns.
- **≤600px** (phone):
  - `.card-grid` → 1 column (full-width cards, YouTube-feed style — user-selected).
  - `.page` padding 24px → 16px, vertical margin 32px → 20px.
  - `nav` gap 24px → 14px, slightly reduced padding.
  - `.login-card`: `width: 340px` → `max-width: 340px; width: 100%` with side
    margin so it never overflows narrow screens.

### Filter tabs (≤600px)

One row, horizontally swipeable (YouTube-chips pattern):
`flex-wrap: nowrap; overflow-x: auto; -webkit-overflow-scrolling: touch`,
scrollbar hidden, `.filter-tab { flex-shrink: 0 }`. Negative side margins +
matching padding so the row bleeds edge-to-edge and swiping feels natural.

### Settings table

New `.table-scroll` wrapper with `overflow-x: auto`; `.data-table` gets a
`min-width` so columns stay readable and the wrapper scrolls sideways instead
of the page.

### Touch select-mode (`@media (hover: none)`)

- `.card-checkbox { display: block }` — checkboxes always visible on touch;
  tapping one enters select mode exactly as the existing JS already handles.
- Quick-delete (`.card-delete-btn`) stays hover-only; on touch, deletion goes
  through select mode or the detail modal's Delete button (both already work).
- `.bulk-toolbar { flex-wrap: wrap }` so its four controls fit narrow screens.
- Tap targets: filter tabs and pagination buttons get ≥40px min-height under
  the same query.

### Modals (≤600px)

`.modal-box` already `width: 90%`; add `max-height: 90dvh; overflow-y: auto`
and slightly reduced `.detail-modal-body` padding so long content scrolls
within the modal.

## Out of scope

- Backend / API changes.
- JS behavior changes.
- The browser-extension pages (`extension/`).
- Restructuring the settings table into stacked cards.

## Testing

- Devtools responsive mode across all four pages at 360px, 600px, 900px,
  desktop widths.
- Touch emulation (or real device): verify checkbox visible, select mode +
  bulk delete reachable, filter tabs swipe, settings table swipes, modals
  scroll.
- Desktop regression check: layouts unchanged at >900px.

# Adjustable YouTube playlist preview limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let dashboard users request up to 200 YouTube playlist entries while preserving the 50-entry default.

**Architecture:** Extend the authenticated playlist-preview request with an optional validated limit. Keep the parser limit-driven and have `yt-dlp` receive the same value. Add a numeric reload control to the existing playlist modal that remembers the submitted playlist URL.

**Tech Stack:** Rust, Axum, Tokio process execution, serde, HTML/CSS/vanilla JavaScript.

## Global Constraints

- Default playlist preview limit remains 50.
- User-requested limits must be integers from 1 through 200.
- Invalid limits must be rejected before running `yt-dlp`.
- Existing playlist selection and status behavior must remain unchanged.

### Task 1: Parameterize and validate playlist preview limits

**Files:**
- Modify: `server/src/handlers/playlist.rs`
- Modify: `server/src/main.rs`

- [ ] Add tests for the default limit, a custom limit, and invalid limits.
- [ ] Run the focused Rust tests and confirm the new tests fail for the missing behavior.
- [ ] Add request parsing and server-side validation, parameterize entry truncation and `yt-dlp --playlist-end`, and retain the 50 default.
- [ ] Run the focused Rust tests and confirm they pass.

### Task 2: Add the dashboard limit control

**Files:**
- Modify: `server/templates/dashboard.html`
- Modify: `server/static/style.css`

- [ ] Add a compact numeric field to the playlist toolbar with a 50 default and 200 maximum.
- [ ] Track the playlist URL, submit the requested limit on Enter, show loading/error state, and refresh the modal payload.
- [ ] Preserve existing selection controls and make the new control responsive.
- [ ] Run the full test suite and inspect the final diff.

### Task 3: Integrate and verify

- [ ] Run the full test suite.
- [ ] Confirm only the intended feature files changed.
- [ ] Commit the feature and merge it to `main`.
- [ ] Run the full test suite on the merged result and confirm a clean working tree.

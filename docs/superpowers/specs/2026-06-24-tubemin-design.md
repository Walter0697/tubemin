# Tubemin — Design Spec

**Date:** 2026-06-24
**Status:** Approved

## Overview

Tubemin is a self-hosted media archiving pipeline. A Chrome extension lets you send any MeTube-supported URL to your home server with a single click. The server queues the download through MeTube, watches for completion, and automatically moves the finished file into PeerTube's watched folder for auto-import. A web dashboard provides submission history and API key management.

The name is Tubemin (YouTube + Pikmin). All user-facing copy refers to "MeTube-supported URLs" rather than any specific platform, and the actual downloading is fully delegated to MeTube (yt-dlp). Tubemin orchestrates; it does not download.

---

## Repository Structure

Single monorepo — one clone, one `docker compose up`.

```
tubemin/
├── extension/           # Chrome extension (Manifest V3)
├── server/              # Rust/Axum server
├── docker-compose.yml
├── example.env
└── README.md
```

---

## Architecture

```
Chrome Extension
      │  POST /api/submit  (X-API-Key header)
      ▼
┌──────────────────────────────┐
│      Tubemin Server (Rust)   │
│  POST /api/submit            │──► MeTube internal API
│  GET  /dashboard  (OIDC)     │
│  GET  /settings   (OIDC)     │
│  Background: folder watcher  │
└──────────────────────────────┘
           │ shared volume: /downloads
           ▼
      ┌─────────┐       ┌──────────┐
      │  MeTube │       │ PeerTube │
      └─────────┘       └──────────┘
      /downloads ──move──► /peertube-import (watched folder)
```

**Docker Compose services:**
- `tubemin` — the Rust server; mounts `/downloads` (read) and `/peertube-import` (write)
- `metube` — mounts `/downloads` as its output directory
- `peertube` — mounts `/peertube-import` as its watched folder (auto-import enabled in admin settings)
- `caddy` — TLS termination and reverse proxy routing external traffic

---

## Data Flow

1. User clicks "Send to Tubemin" in the extension popup.
2. Extension POSTs `{"url": "<media-url>"}` to `POST https://<domain>/api/submit` with `X-API-Key` header.
3. Server validates the API key, calls MeTube's internal `POST http://metube:8081/add` with the URL.
4. Server records the submission in SQLite with status `pending`.
5. MeTube downloads the file to `/downloads`, writing a `.part` file during transfer and renaming to the final filename on completion.
6. Server's background watcher (`notify` crate) detects the rename event on a non-`.part` file.
7. Server moves the file from `/downloads` to `/peertube-import` and updates the SQLite record to `imported`.
8. PeerTube detects the new file in its watched folder and auto-imports it.

---

## Components

### Chrome Extension (Manifest V3)

**Popup (`popup.html`)**
- "Send to Tubemin" button — reads `tabs` API for the active tab URL, POSTs to the configured server.
- Button is disabled (grayed out, with hint text "Configure API key in settings") if server URL or API key is not set.
- Shows inline success ("Queued!") or error ("Failed — check settings") feedback after the request.
- Gear icon links to the settings tab.

**Settings page (`settings.html`)**
- Server URL input field.
- API key input field — once saved, displays masked (`••••••••1234`) with a "Change" button that re-enables the field.
- Both values stored in `chrome.storage.sync` so they roam across the user's Chrome profile.
- Always accessible regardless of whether values are set.

**Permissions:** `activeTab`, `storage`.

---

### Tubemin Server (Rust / Axum)

#### Endpoints

**`POST /api/submit`**
- Auth: `X-API-Key` header validated against stored key hash.
- Body: `{"url": "<string>"}`.
- Calls `POST http://metube:8081/add` internally with `{"url": "...", "folder": "/downloads"}`.
- Records submission in SQLite: `id`, `url`, `submitted_at`, `status` (`pending` | `downloaded` | `imported` | `error`).
- Returns `200 {"status": "queued"}` or `4xx`/`5xx` on failure.

**`GET /dashboard`**
- Auth: OIDC session cookie.
- Returns submission history table: URL, submitted time, current status.
- No write operations — read-only view.

**`GET /settings`**
- Auth: OIDC session cookie.
- Shows active API keys with `created_at` and `last_used_at`.
- Actions: generate new key (returns plaintext once), revoke existing key.

#### Background Watcher
- Spawned as a Tokio task on server startup.
- Uses `notify` crate to watch `/downloads` directory recursively.
- On `EventKind::Create` or rename-to event for a file that does not end in `.part` or `.tmp`: moves it to `/peertube-import` and updates the SQLite record matching the filename to `imported`.
- If no matching record exists (e.g. manual MeTube add), still moves the file; skips the DB update.

#### Database (SQLite via sqlx)
```sql
CREATE TABLE submissions (
    id          TEXT PRIMARY KEY,  -- UUID
    url         TEXT NOT NULL,
    filename    TEXT,              -- set when MeTube picks it up (optional)
    status      TEXT NOT NULL DEFAULT 'pending',
    submitted_at TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE api_keys (
    id          TEXT PRIMARY KEY,
    key_hash    TEXT NOT NULL,     -- bcrypt hash, never store plaintext
    created_at  TEXT NOT NULL,
    last_used_at TEXT
);
```

#### Configuration (env vars)
```
API_PORT=3000
METUBE_URL=http://metube:8081
DOWNLOADS_DIR=/downloads
PEERTUBE_IMPORT_DIR=/peertube-import
DATABASE_URL=sqlite:///data/tubemin.db
OIDC_ISSUER_URL=https://...
OIDC_CLIENT_ID=...
OIDC_CLIENT_SECRET=...
```

---

### Docker Compose

```yaml
services:
  caddy:
    image: caddy:alpine
    ports: ["80:80", "443:443"]
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile

  tubemin:
    build: ./server
    env_file: .env
    volumes:
      - downloads:/downloads
      - peertube_import:/peertube-import
      - tubemin_data:/data
    depends_on: [metube]

  metube:
    image: ghcr.io/alexta69/metube
    volumes:
      - downloads:/downloads
    environment:
      DOWNLOAD_DIR: /downloads

  peertube:
    image: chocobozzz/peertube:production-bookworm
    volumes:
      - peertube_import:/peertube-import
      # ... standard PeerTube volumes

volumes:
  downloads:
  peertube_import:
  tubemin_data:
```

---

## Auth

**Chrome Extension → Server:** `X-API-Key` header. Key is generated in `/settings`, stored hashed (bcrypt) in SQLite. The extension stores the plaintext key in `chrome.storage.sync`.

**Dashboard / Settings:** OIDC (e.g. Authentik, Authelia, or any compliant provider). Caddy or the Rust server handles the OIDC redirect flow. Only `/dashboard` and `/settings` routes require OIDC — `/api/submit` uses only the API key.

---

## Error Handling

| Scenario | Behavior |
|---|---|
| MeTube unreachable | Server returns `503`, submission not recorded |
| Invalid/missing API key | Server returns `401` |
| File move fails (permissions, disk full) | Log error, update status to `error`, do not delete source file |
| PeerTube watched folder not mounted | Move fails gracefully, status set to `error`, operator notified via log |
| Duplicate URL submitted | Accepted and queued again — MeTube deduplication is out of scope |

---

## Out of Scope (for v1)

- Push notifications to the extension when a download completes
- Multiple user accounts (single-operator tool)
- PeerTube channel selection from the extension
- Retry logic for failed moves
- MeTube queue status polling (watcher is sufficient)

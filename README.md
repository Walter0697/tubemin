# Tubemin

A self-hosted video pipeline: submit YouTube (or any yt-dlp-supported) URL from a Chrome extension → downloaded by MeTube → auto-imported into PeerTube.

```
Chrome extension  →  Tubemin API  →  MeTube  →  /downloads  →  PeerTube
```

## Components

| Service | Role |
|---------|------|
| **Tubemin** | Rust API server + web dashboard |
| **MeTube** | yt-dlp frontend that does the actual downloading |
| **PeerTube** | Self-hosted video platform, receives imported videos |
| **Caddy** | Reverse proxy with automatic HTTPS |
| **Chrome extension** | One-click submit from any browser tab |

## Quick start (local, no HTTPS)

Good for testing. Skips Caddy; Tubemin listens directly on port 3000.

```bash
cp example.env .env
# Set AUTH_MODE=password and ADMIN_PASSWORD in .env
docker compose -f docker-compose.yml -f docker-compose.local.yml up --build
```

Then open `http://localhost:3000`.

## Production setup

### 1. DNS

Point two domains at your server:

- `tubemin.yourdomain.com` → Tubemin dashboard
- `peertube.yourdomain.com` → PeerTube

### 2. Configure `.env`

```bash
cp example.env .env
```

Edit `.env`. Required values:

```bash
# Auth — pick one mode
AUTH_MODE=oidc          # or: password

# If AUTH_MODE=password
ADMIN_PASSWORD=strong-password-here

# If AUTH_MODE=oidc (Authentik, Authelia, Keycloak, etc.)
OIDC_ISSUER_URL=https://auth.yourdomain.com/application/o/tubemin/
OIDC_CLIENT_ID=tubemin
OIDC_CLIENT_SECRET=your-client-secret
OIDC_REDIRECT_URL=https://tubemin.yourdomain.com/auth/callback

# Caddy domains
TUBEMIN_DOMAIN=tubemin.yourdomain.com
PEERTUBE_DOMAIN=peertube.yourdomain.com

# PeerTube init
PEERTUBE_DB_PASSWORD=strong-db-password
PEERTUBE_SECRET=$(openssl rand -hex 32)
PEERTUBE_ADMIN_EMAIL=admin@yourdomain.com
PEERTUBE_ADMIN_PASSWORD=strong-admin-password
PEERTUBE_WEBSERVER_HOSTNAME=peertube.yourdomain.com
PEERTUBE_WEBSERVER_PORT=443
PEERTUBE_WEBSERVER_HTTPS=true

# PeerTube upload bot (auto-created by Tubemin on first start)
PEERTUBE_URL=http://peertube:9000
PEERTUBE_HOST=peertube.yourdomain.com
PEERTUBE_USERNAME=tubemin-bot
PEERTUBE_PASSWORD=strong-bot-password
```

### 3. Start

```bash
docker compose up -d
```

Caddy obtains TLS certificates automatically. First startup takes ~2 minutes for PeerTube to become healthy before Tubemin connects.

### 4. Generate an API key

Open `https://tubemin.yourdomain.com/settings`, log in, and generate a key. You'll enter this in the Chrome extension settings.

## Chrome extension

Load unpacked from the `extension/` directory in `chrome://extensions` (Developer mode on).

In the extension settings, enter:
- **Server URL**: `https://tubemin.yourdomain.com` (no trailing slash)
- **API Key**: the key you generated in step 4

Click the extension icon on any video page to submit it.

## Auth modes

### Password

Single shared password set via `ADMIN_PASSWORD`. Simple, no external dependencies.

### OIDC

Delegates login to an external provider (Authentik, Authelia, Keycloak, etc.). Set up an OIDC application in your provider with:

- **Redirect URI**: `https://tubemin.yourdomain.com/auth/callback`
- **Scopes**: `openid profile email`

Then fill in the four `OIDC_*` vars in `.env`.

## Pipeline details

1. Extension POSTs the URL to `/api/submit` (requires API key).
2. Tubemin validates the URL against the yt-dlp supported-domain list and forwards it to MeTube.
3. MeTube downloads the video to the shared `/downloads` volume.
4. Tubemin's file watcher detects the new file and triggers a PeerTube import via the API.
5. The dashboard auto-refreshes every 5 seconds while any submission is pending.

**Status flow**: `pending` → `imported` (success) or `error` (yt-dlp failed)

## Rebuilding after changes

```bash
docker compose up --build --pull never -d tubemin
```

## Environment variable reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `AUTH_MODE` | no | `oidc` | `oidc` or `password` |
| `ADMIN_PASSWORD` | if password mode | — | Dashboard login password |
| `OIDC_ISSUER_URL` | if OIDC mode | — | Provider discovery URL |
| `OIDC_CLIENT_ID` | if OIDC mode | — | OAuth client ID |
| `OIDC_CLIENT_SECRET` | if OIDC mode | — | OAuth client secret |
| `OIDC_REDIRECT_URL` | if OIDC mode | — | Must be `https://<domain>/auth/callback` |
| `API_PORT` | no | `3000` | Internal port Tubemin listens on |
| `DATABASE_URL` | yes | — | `sqlite:///data/tubemin.db` |
| `METUBE_URL` | no | `http://metube:8081` | MeTube internal address |
| `DOWNLOADS_DIR` | no | `/downloads` | Where MeTube saves files |
| `PEERTUBE_IMPORT_DIR` | no | `/peertube-import` | PeerTube watched folder |
| `PEERTUBE_URL` | no | — | PeerTube internal address (enables upload) |
| `PEERTUBE_HOST` | no | — | PeerTube public hostname (for Host header) |
| `PEERTUBE_USERNAME` | no | — | Bot account username |
| `PEERTUBE_PASSWORD` | no | — | Bot account password |
| `PEERTUBE_ADMIN_EMAIL` | no | — | Used to provision bot account |
| `PEERTUBE_ADMIN_USERNAME` | no | `root` | PeerTube admin username |
| `PEERTUBE_ADMIN_PASSWORD` | no | — | PeerTube admin password (for bot provisioning) |
| `TUBEMIN_DOMAIN` | yes (prod) | — | Caddy HTTPS domain for Tubemin |
| `PEERTUBE_DOMAIN` | yes (prod) | — | Caddy HTTPS domain for PeerTube |

# Tubemin

<img src="extension/icons/icon128.png" width="64" align="right" alt="Tubemin icon">

A self-hosted video pipeline: submit any MeTube-supported URL from a Chrome extension → downloaded by MeTube → auto-imported into PeerTube.

```
Chrome extension  →  Tubemin API  →  MeTube  →  /downloads  →  PeerTube
```

## Components

| Service | Role |
|---------|------|
| **Tubemin** | Rust API server + web dashboard |
| **MeTube** | yt-dlp frontend that does the actual downloading |
| **PeerTube** | Self-hosted video platform, receives imported videos |
| **Chrome extension** | One-click submit from any browser tab |

## Local development

This repository contains the TubeMin application and does not contain the
deployment Compose files. Keep the machine-specific Compose stack, `.env`,
bind mounts, and secrets in the deployment directory (for example,
`/home/services/tubemin`). Build or run the application from there using the
deployment instructions for that machine.

### Machine-to-machine service accounts

Tubemin can expose PeerTube-owned media to trusted consumers such as ConvertTube without sharing a user API key. Define service accounts in the read-only `service-accounts/service-accounts.toml` startup manifest and provide each token through an environment variable or Docker secret:

```toml
[[service_accounts]]
name = "converttube"
token_env = "TUBEMIN_CONVERTTUBE_TOKEN"
scopes = ["catalog:read", "media:read", "transfer:complete", "transfer:fail"]
enabled = true
```

The service API provides a catalog, video metadata, original media, thumbnails, completion/failure callbacks, and transfer history. A successful completion records the destination, removes Tubemin's local source artifacts, deletes the bot-owned PeerTube video, and keeps an audit record. Failed jobs leave the source untouched. The transfer history is available in the dashboard at `/transfers`.

The service-account token is only used for machine-to-machine access; existing user-facing API keys remain available for the extension and other user clients.

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
cd /home/services/tubemin
docker compose up -d
```

Caddy obtains TLS certificates automatically. First startup takes ~2 minutes for PeerTube to become healthy before Tubemin connects.

### 4. Generate an API key

Open `https://tubemin.yourdomain.com/settings`, log in, and generate a key. You can set a label there, for example `extension` or `ios-shortcut`. You'll enter this in the extension or shortcut settings.

## Browser extension

The extension is intentionally not listed in any browser store. Install it locally: use developer mode in Chromium browsers, or temporary debugging install in Firefox-based browsers such as Zen.

### Chrome / Chromium installation

**1. Clone the repo** (if you haven't already):

```bash
git clone https://github.com/Walter0697/tubemin.git
```

**2. Open Chrome extensions page:**

Navigate to `chrome://extensions` in your browser.

**3. Enable Developer mode:**

Toggle **Developer mode** on (top-right corner of the extensions page).

**4. Load the extension:**

Click **Load unpacked**, then select the `extension/` folder inside the cloned repo.

The Tubemin icon will appear in your toolbar. Pin it for easy access.

### Firefox / Zen installation

Firefox-based browsers do not support Chrome-style unpacked loading from `about:addons`. Use temporary extension loading instead.

**1. Clone the repo** (if you haven't already):

```bash
git clone https://github.com/Walter0697/tubemin.git
```

**2. Open the debugging page:**

Navigate to `about:debugging#/runtime/this-firefox` in Firefox or Zen.

**3. Load the extension temporarily:**

Click **Load Temporary Add-on**, open the repo's `extension/` folder, and select `manifest.json`.

You can also temporarily load the packaged `tubemin-firefox-extension.xpi` artifact from a release, but temporary loading from the repo is the simplest development workflow.

**4. Reload after browser restart:**

Temporary add-ons are removed when the browser fully restarts. Re-open `about:debugging` and load it again when needed.

### Configuration

Click the extension icon -> **Settings**, then enter:

- **Server URL**: `https://tubemin.yourdomain.com` (or `http://localhost:3000` for local)
- **API Key**: the key generated in Tubemin's Settings page
- **Minimum video duration** *(optional)*: ignore clips shorter than N minutes (applies to HLS streams only)

Click **Save**, then **Test Connection** to verify.

### Keeping it updated

For Chrome/Chromium, the extension loads directly from the cloned folder, so a `git pull` is all you need. If the `manifest.json` changes, go to `chrome://extensions` and click the **↺ reload** icon on the Tubemin card.

For Firefox/Zen temporary installs, reload it from `about:debugging` after code changes, and re-add it after a full browser restart.

### Usage

- **MeTube-supported sites** (e.g. Vimeo, Twitch, etc.): navigate to the video page and click the extension icon → **Queue Video**.
- **Other sites**: play the video first so the player makes its network requests, then click the extension icon. Detected streams appear as a list — rename if needed, select the ones you want, click **Queue Selected**.

## Auth modes

### Password

Single shared password set via `ADMIN_PASSWORD`. Simple, no external dependencies.

### OIDC

Delegates login to an external provider (Authentik, Authelia, Keycloak, etc.). Set up an OIDC application in your provider with:

- **Redirect URI**: `https://tubemin.yourdomain.com/auth/callback`
- **Scopes**: `openid profile email`

Then fill in the four `OIDC_*` vars in `.env`.

## Pipeline details

1. Extension POSTs the URL to `/api/submit` (requires API key). The body can include an optional `source` field such as `extension` or `ios-shortcut`.
2. Tubemin validates the URL and forwards it to MeTube (or downloads directly via ffmpeg for raw stream URLs).
3. MeTube downloads the video to the shared `/downloads` volume.
4. Tubemin's file watcher detects the new file and triggers a PeerTube import via the API.
5. The dashboard auto-refreshes every 5 seconds while any submission is pending.

**Status flow**: `pending` → `imported` (success) or `error` (download failed)

## Rebuilding after changes

```bash
cd /home/services/tubemin
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
| `OIDC_LOGIN_LABEL` | no | `Sign in with Authentik` | Login button label on the sign-in page |
| `API_PORT` | no | `3000` | Internal port Tubemin listens on |
| `DATABASE_URL` | yes | — | `sqlite:///data/tubemin.db` |
| `TUBEMIN_SERVICE_ACCOUNTS_FILE` | no | `/data/service-accounts.toml` | Startup TOML manifest defining machine-to-machine accounts |
| `METUBE_URL` | no | `http://metube:8081` | MeTube internal address |
| `METUBE_UPDATE_URL` | no | — | Optional internal MeTube maintenance endpoint for yt-dlp update/restart |
| `METUBE_UPDATE_TOKEN` | no | — | Bearer token for the optional MeTube maintenance endpoint |
| `METUBE_AUTO_UPDATE_ON_FAILURE` | no | `false` | Request a MeTube yt-dlp update before retrying eligible download failures |
| `DOWNLOADS_DIR` | no | `/downloads` | Where MeTube saves files |
| `PEERTUBE_IMPORT_DIR` | no | `/peertube-import` | PeerTube watched folder |
| `PEERTUBE_URL` | no | — | PeerTube internal address (enables upload) |
| `PEERTUBE_HOST` | no | — | PeerTube public hostname (for Host header) |
| `PEERTUBE_USERNAME` | no | — | Bot account username |
| `PEERTUBE_PASSWORD` | no | — | Bot account password |
| `TUBEMIN_PEERTUBE_CLEANUP_TOKEN` | no | — | Dedicated bearer token for Jellypik's post-migration cleanup endpoint |
| `PEERTUBE_ADMIN_EMAIL` | no | — | Used to provision bot account |
| `PEERTUBE_ADMIN_USERNAME` | no | `root` | PeerTube admin username |
| `PEERTUBE_ADMIN_PASSWORD` | no | — | PeerTube admin password (for bot provisioning) |
| `PEERTUBE_VIDEO_PRIVACY` | no | `4` | Upload privacy: 1=Public 2=Unlisted 3=Private 4=Internal |
| `PEERTUBE_OIDC_ISSUER_URL` | no | — | Authentik issuer URL for the PeerTube OIDC plugin |
| `PEERTUBE_OIDC_CLIENT_ID` | no | — | Client ID for the PeerTube Authentik application |
| `PEERTUBE_OIDC_CLIENT_SECRET` | no | — | Client secret for the PeerTube Authentik application |
| `TUBEMIN_DOMAIN` | yes (prod) | — | Caddy HTTPS domain for Tubemin |
| `PEERTUBE_DOMAIN` | yes (prod) | — | Caddy HTTPS domain for PeerTube |

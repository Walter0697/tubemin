---
name: rebuild
description: Use when tubemin server code, templates, or static files have changed and the Docker container needs to be rebuilt and restarted to reflect those changes.
---

# Rebuild & Restart Tubemin

Rebuild the Docker image and restart the container. Always run from the tubemin project root.

## Commands

```bash
docker compose build tubemin && docker compose up -d tubemin
```

## When to remind the user

After rebuilding, remind the user to **reload the Chrome extension** (`chrome://extensions`) if any extension files changed (`extension/`).

## Notes

- `build` recompiles the Rust binary and bakes in templates/static files
- `up -d` recreates only the `tubemin` container; leaves MeTube, PeerTube, Caddy untouched
- If PeerTube was also stopped (e.g. full `docker compose down`), use `docker compose up -d` to bring everything up

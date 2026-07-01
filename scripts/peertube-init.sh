#!/bin/sh
# One-shot PeerTube init: installs + configures OIDC plugin, sets requiresAuth.
# Runs inside an alpine container after PeerTube passes its health check.
# Safe to re-run (idempotent).
set -eu

apk add --no-cache curl jq > /dev/null

PT_URL="${PEERTUBE_URL:-http://peertube:9000}"
ADMIN_USER="${PEERTUBE_ADMIN_USERNAME:-root}"
ADMIN_PASS="${PEERTUBE_ADMIN_PASSWORD}"
OIDC_ISSUER="${PEERTUBE_OIDC_ISSUER_URL}"
OIDC_CLIENT_ID="${PEERTUBE_OIDC_CLIENT_ID}"
OIDC_CLIENT_SECRET="${PEERTUBE_OIDC_CLIENT_SECRET}"
PLUGIN="peertube-plugin-auth-openid-connect"

echo "[peertube-init] Authenticating as ${ADMIN_USER}..."

OAUTH=$(curl -sf "${PT_URL}/api/v1/oauth-clients/local")
CLIENT_ID=$(echo "$OAUTH" | jq -r '.client_id')
CLIENT_SECRET=$(echo "$OAUTH" | jq -r '.client_secret')

TOKEN=$(curl -sf \
  --data-urlencode "client_id=${CLIENT_ID}" \
  --data-urlencode "client_secret=${CLIENT_SECRET}" \
  --data-urlencode "grant_type=password" \
  --data-urlencode "response_type=code" \
  --data-urlencode "username=${ADMIN_USER}" \
  --data-urlencode "password=${ADMIN_PASS}" \
  "${PT_URL}/api/v1/users/token" | jq -r '.access_token')
# Access token TTL is ~1 hour. The script normally completes in seconds,
# but a stall (e.g. npm network issue) could cause later API calls to 401.
# If that happens, re-run the container: docker compose run --rm peertube_init

echo "[peertube-init] Installing plugin ${PLUGIN}..."

HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "{\"npmName\":\"${PLUGIN}\"}" \
  "${PT_URL}/api/v1/plugins/install")

# 204 = installed, 409 = already installed — both are fine
if [ "$HTTP_STATUS" != "200" ] && [ "$HTTP_STATUS" != "204" ] && [ "$HTTP_STATUS" != "409" ]; then
  echo "[peertube-init] Plugin install failed with HTTP ${HTTP_STATUS}" >&2
  exit 1
fi
echo "[peertube-init] Plugin ready (HTTP ${HTTP_STATUS})"

echo "[peertube-init] Configuring OIDC plugin..."

curl -sf \
  -X PUT \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "$(jq -n \
    --arg url  "$OIDC_ISSUER" \
    --arg cid  "$OIDC_CLIENT_ID" \
    --arg csec "$OIDC_CLIENT_SECRET" \
    '{settings: {
        discoveryUrl:    $url,
        clientId:        $cid,
        clientSecret:    $csec,
        scope:           "openid email profile",
        authDisplayName: "Login with Authentik",
        autoRegistration: true
    }}')" \
  "${PT_URL}/api/v1/plugins/${PLUGIN}/settings"

echo "[peertube-init] Plugin configured"

echo "[peertube-init] Setting instance requiresAuth=true..."

CURRENT=$(curl -sf \
  -H "Authorization: Bearer ${TOKEN}" \
  "${PT_URL}/api/v1/config/custom")

UPDATED=$(echo "$CURRENT" | jq '.instance.requiresAuth = true')

curl -sf \
  -X PUT \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "$UPDATED" \
  "${PT_URL}/api/v1/config/custom"

echo "[peertube-init] Done — PeerTube requires login and OIDC is configured."

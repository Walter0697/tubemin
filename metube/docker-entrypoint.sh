#!/bin/sh
set -eu

PUID="${UID:-${PUID:-911}}"
PGID="${GID:-${PGID:-911}}"
AUDIO_DOWNLOAD_DIR="${AUDIO_DOWNLOAD_DIR:-$DOWNLOAD_DIR}"
UPDATE_REQUEST_FILE=/tmp/metube-update-request
APP_PID_FILE=/tmp/metube-app.pid

umask "${UMASK:-022}"
mkdir -p "${DOWNLOAD_DIR}" "${AUDIO_DOWNLOAD_DIR}" "${STATE_DIR:-/app/medias}" "${TEMP_DIR:-/tmp}"

do_upgrade() {
    echo "Upgrading yt-dlp on manual request..."
    if ! python3 -m pip --version >/dev/null 2>&1; then
        python3 -m ensurepip --upgrade >/dev/null 2>&1 || true
    fi
    python3 -m pip install -U --pre "yt-dlp[default,curl-cffi,deno]" || {
        echo "Warning: yt-dlp upgrade failed; continuing with existing installation"
        return 1
    }
    echo "yt-dlp upgrade complete"
}

run_supervised() {
    while true; do
        rm -f "${UPDATE_REQUEST_FILE}"
        "$@" &
        child_pid=$!
        echo "${child_pid}" > "${APP_PID_FILE}"
        trap 'kill -TERM "$child_pid" 2>/dev/null || true; wait "$child_pid" 2>/dev/null || true' TERM INT
        exit_code=0
        wait "$child_pid" || exit_code=$?
        rm -f "${APP_PID_FILE}"
        trap - TERM INT
        if [ -f "${UPDATE_REQUEST_FILE}" ] || [ "${exit_code}" -eq 42 ]; then
            rm -f "${UPDATE_REQUEST_FILE}"
            do_upgrade || true
            continue
        fi
        return "${exit_code}"
    done
}

python3 /app/tubemin-maintenance.py &
maintenance_pid=$!
trap 'kill "$maintenance_pid" 2>/dev/null || true' EXIT TERM INT

if [ "$(id -u)" -eq 0 ] && [ "$(id -g)" -eq 0 ]; then
    if [ "${CHOWN_DIRS:-true}" != "false" ]; then
        chown -R "${PUID}:${PGID}" /app "${DOWNLOAD_DIR}" "${AUDIO_DOWNLOAD_DIR}" "${STATE_DIR:-/app/medias}" "${TEMP_DIR:-/tmp}"
    fi
    if [ -n "${YTDL_NIGHTLY_UPDATE_TIME:-}" ]; then
        do_upgrade || true
    fi
    bgutil-pot server >/tmp/bgutil-pot.log 2>&1 &
    run_supervised gosu "${PUID}:${PGID}" python3 app/main.py
else
    unset YTDL_NIGHTLY_UPDATE_TIME
    bgutil-pot server >/tmp/bgutil-pot.log 2>&1 &
    run_supervised python3 app/main.py
fi

#!/usr/bin/env bash
set -u

# Conservative weekly cleanup for rebuildable cache data only.
# Run with DRY_RUN=1 to print actions without deleting.

HOME_DIR="${HOME:?HOME is not set}"
LOG_DIR="$HOME_DIR/Library/Logs"
LOG_FILE="${LOG_FILE:-$LOG_DIR/weekly-cache-cleanup.log}"
DRY_RUN="${DRY_RUN:-0}"

export PATH="/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

mkdir -p "$(dirname "$LOG_FILE")" 2>/dev/null || true

log() {
  local line
  line="[$(date '+%Y-%m-%d %H:%M:%S')] $*"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$LOG_FILE" 2>/dev/null || true
}

run() {
  log "+ $*"
  if [ "$DRY_RUN" = "1" ]; then
    return 0
  fi
  "$@" >>"$LOG_FILE" 2>&1 || log "command failed with status $?: $*"
}

remove_path() {
  local path="$1"

  case "$path" in
    "$HOME_DIR"/Library/Caches/*|"$HOME_DIR"/Library/Developer/Xcode/DerivedData)
      ;;
    *)
      log "skip unsafe path: $path"
      return 0
      ;;
  esac

  if [ -e "$path" ]; then
    run rm -rf "$path"
  else
    log "skip missing: $path"
  fi
}

log "weekly cleanup started; dry_run=$DRY_RUN"

if command -v docker >/dev/null 2>&1; then
  run docker builder prune -af
else
  log "skip docker: command not found"
fi

if command -v xcrun >/dev/null 2>&1; then
  run xcrun simctl delete unavailable
else
  log "skip simctl: xcrun not found"
fi

remove_path "$HOME_DIR/Library/Developer/Xcode/DerivedData"

remove_path "$HOME_DIR/Library/Caches/com.openai.codex"
remove_path "$HOME_DIR/Library/Caches/com.todesktop.230313mzl4w4u92.ShipIt"
remove_path "$HOME_DIR/Library/Caches/Google"
remove_path "$HOME_DIR/Library/Caches/typescript"
remove_path "$HOME_DIR/Library/Caches/Firefox"
remove_path "$HOME_DIR/Library/Caches/pnpm"
remove_path "$HOME_DIR/Library/Caches/Mozilla"
remove_path "$HOME_DIR/Library/Caches/CocoaPods"
remove_path "$HOME_DIR/Library/Caches/org.whispersystems.signal-desktop.ShipIt"
remove_path "$HOME_DIR/Library/Caches/notion.id.ShipIt"
remove_path "$HOME_DIR/Library/Caches/Adobe"
remove_path "$HOME_DIR/Library/Caches/node-gyp"
remove_path "$HOME_DIR/Library/Caches/Homebrew"
remove_path "$HOME_DIR/Library/Caches/pip"

if command -v pnpm >/dev/null 2>&1; then
  run pnpm store prune
else
  log "skip pnpm prune: command not found"
fi

if command -v npm >/dev/null 2>&1; then
  run npm cache verify
else
  log "skip npm cache verify: command not found"
fi

if command -v brew >/dev/null 2>&1; then
  run brew cleanup -s
else
  log "skip brew cleanup: command not found"
fi

run df -h /System/Volumes/Data
log "weekly cleanup finished"

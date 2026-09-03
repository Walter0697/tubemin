#!/usr/bin/env bash
set -euo pipefail

# Remove only PeerTube videos that are not tracked by Tubemin.
# Default is a dry run. Pass --apply to delete the printed stale UUIDs.

apply=false
container="tubemin-tubemin-1"
db_path="/data/tubemin.db"
while (($#)); do
  case "$1" in
    --apply) apply=true ;;
    --container) container="$2"; shift ;;
    --db-path) db_path="$2"; shift ;;
    *) echo "usage: $0 [--apply] [--container NAME] [--db-path PATH]" >&2; exit 2 ;;
  esac
  shift
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

docker cp "$container:$db_path" "$tmp_dir/tubemin.db" >/dev/null
sqlite3 "$tmp_dir/tubemin.db" "SELECT peertube_uuid FROM submissions WHERE peertube_uuid IS NOT NULL;" | sort -u >"$tmp_dir/keep.txt"

oauth="$(curl -fsS -m 20 -H 'Host: localhost:7006' http://127.0.0.1:9000/api/v1/oauth-clients/local)"
client_id="$(jq -er .client_id <<<"$oauth")"
client_secret="$(jq -er .client_secret <<<"$oauth")"
token="$(curl -fsS -m 20 -H 'Host: localhost:7006' -X POST http://127.0.0.1:9000/api/v1/users/token \
  --data-urlencode "client_id=$client_id" --data-urlencode "client_secret=$client_secret" \
  --data-urlencode 'grant_type=password' --data-urlencode 'response_type=code' \
  --data-urlencode 'username=tubemin-bot' --data-urlencode 'password=tubemin-local-test-bot' | jq -er .access_token)"

curl -fsS -m 30 -H 'Host: localhost:7006' -H "Authorization: Bearer $token" \
  'http://127.0.0.1:9000/api/v1/videos?start=0&count=100' |
  jq -er '.data[].uuid' | sort -u >"$tmp_dir/all.txt"
comm -23 "$tmp_dir/all.txt" "$tmp_dir/keep.txt" >"$tmp_dir/stale.txt"

echo "Tubemin keep set: $(wc -l <"$tmp_dir/keep.txt") video(s)"
echo "PeerTube catalog: $(wc -l <"$tmp_dir/all.txt") video(s)"
echo "Stale candidates: $(wc -l <"$tmp_dir/stale.txt") video(s)"
cat "$tmp_dir/stale.txt"

if ! $apply; then
  echo "Dry run only. Re-run with --apply after reviewing the stale UUIDs."
  exit 0
fi

# Re-read both sets immediately before deletion. This prevents a stale
# snapshot from deleting a video that Tubemin began tracking meanwhile.
docker cp "$container:$db_path" "$tmp_dir/tubemin-final.db" >/dev/null
sqlite3 "$tmp_dir/tubemin-final.db" "SELECT peertube_uuid FROM submissions WHERE peertube_uuid IS NOT NULL;" | sort -u >"$tmp_dir/keep-final.txt"
curl -fsS -m 30 -H 'Host: localhost:7006' -H "Authorization: Bearer $token" \
  'http://127.0.0.1:9000/api/v1/videos?start=0&count=100' |
  jq -er '.data[].uuid' | sort -u >"$tmp_dir/all-final.txt"
comm -23 "$tmp_dir/all-final.txt" "$tmp_dir/keep-final.txt" >"$tmp_dir/stale-final.txt"

while IFS= read -r uuid; do
  [[ -z "$uuid" ]] && continue
  curl -fsS -m 30 -X DELETE -H 'Host: localhost:7006' -H "Authorization: Bearer $token" \
    "http://127.0.0.1:9000/api/v1/videos/$uuid" >/dev/null
  echo "Deleted stale PeerTube video $uuid"
done <"$tmp_dir/stale-final.txt"

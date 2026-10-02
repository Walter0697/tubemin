# Playlist Picker Design

## Goal

Let a dashboard user paste a YouTube URL that belongs to a playlist and choose which playlist videos to queue, without ever downloading a whole playlist by accident.

## Problem

Tubemin tracks every submission by the exact URL it received. A playlist URL creates one submission row, but MeTube reports any expanded entries under their own video URLs. The poller and watcher cannot match those entries back to a row, so at most one video is tracked correctly. The duplicate check also compares exact URL strings, so the same YouTube video submitted as `watch?v=X`, `watch?v=X&list=Y`, `watch?v=X&t=30`, or `youtu.be/X` is not recognised as a duplicate.

## Scope

- Only the web dashboard gets the picker.
- Playlists are not a stored concept: each chosen video becomes an ordinary single-video submission and shows as a normal dashboard row.
- The service catalog and ConvertTube are unchanged.
- The browser extension and iOS Shortcut are unchanged; the server makes their playlist URLs safe.

## Design

### YouTube URL normalisation and video key

`url_validator` gains YouTube helpers:

- `youtube_video_id(url)` extracts the video ID from `watch?v=`, `youtu.be/`, `shorts/`, `embed/`, `live/`, and the `m.`, `music.`, and `www.` hosts.
- `youtube_playlist_id(url)` extracts the `list` parameter.
- `video_key(url)` returns `youtube:<id>` for YouTube videos and `None` for everything else.
- `canonical_youtube_url(id)` returns `https://www.youtube.com/watch?v=<id>`.

Migration `014` adds a nullable, indexed `video_key` column to `submissions`. `db::init` backfills the key for existing rows whose key is still null. New submissions store the key.

### Submission path (all clients)

Before validation, `enqueue` normalises YouTube URLs:

- A URL with a video ID is rewritten to the canonical single-video URL. This drops `list`, `index`, `t`, and other parameters, so MeTube always receives one video and the stored URL matches MeTube's history.
- A playlist-only URL (a `list` parameter and no video ID) is rejected with `422` and a message to use the dashboard playlist picker.

The active-duplicate check matches on `video_key` when the URL has one, and on the exact URL otherwise. Active statuses stay `pending`, `downloading`, `imported`, `transcoding`, and `complete`. Retrying an `error` or `interrupted` row keeps the existing exact-URL behaviour.

### Preview endpoint

`POST /api/playlist/preview` (session auth) takes `{ "url": "..." }` and runs:

```
yt-dlp --flat-playlist --dump-single-json --playlist-end 50 --no-warnings <url>
```

with a 60 second timeout. The response contains:

- `title`, `playlist_id`;
- `selected_video_id`: the video named in the submitted URL, if any;
- `truncated`: true when the playlist reports more than 50 entries or reports no count but returned 50 (for example YouTube Mix lists);
- `entries[]`: `video_id`, `url` (canonical), `title`, `duration`, `thumbnail_url`, `available`, `existing_status`.

`available` is false for entries yt-dlp reports as private, deleted, or members-only. `existing_status` is derived from the latest submission with the same video key:

| Latest status | `existing_status` | Picker default |
|---|---|---|
| pending, downloading, imported, transcoding, complete | `active` | disabled |
| processing, deleted, handed_off | `transferred` | unchecked, selectable |
| error, interrupted | `failed` | unchecked, selectable |
| none | null | checked only if it is `selected_video_id` |

A URL without a playlist ID returns `400`. A yt-dlp failure returns `502` with a short message.

### Dashboard dialog

When the add-video URL contains a playlist ID, the dashboard calls the preview endpoint instead of submitting. A dialog shows the playlist title, a truncation note when relevant, Select all / None buttons, and one row per entry with checkbox, thumbnail, title, duration, and a status tag. Select all only checks enabled entries. Confirming submits each checked entry's canonical URL through the existing `/api/submissions/create` endpoint, then refreshes the list. If the preview fails, the dialog shows the error and offers to add just the selected video.

## Verification

- Unit tests for the YouTube URL helpers, including playlist-only, Mix, `youtu.be`, `shorts`, and non-YouTube URLs.
- Unit tests for parsing yt-dlp flat-playlist JSON (availability, truncation, canonical URLs).
- Handler tests: playlist-only URL rejected; `watch?v=X&list=Y` stored and sent to MeTube as the canonical URL; duplicate detected across URL variants; preview existing-status mapping.
- Full Tubemin test suite, then a manual check of the dialog in the local stack.

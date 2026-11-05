# Adjustable YouTube playlist preview limit

## Goal

Allow dashboard users to request more than the default 50 YouTube playlist entries without making the normal playlist preview expensive by default.

## Design

The playlist preview endpoint accepts an optional `limit` value. The default remains 50; valid custom values are positive integers up to 200. The server rejects invalid values with a client error and passes the validated limit to `yt-dlp --playlist-end`.

The playlist modal adds a compact numeric field. It remembers the original playlist URL, submits the requested limit when the user presses Enter, and replaces the current preview with the larger result. Existing selection, status labels, truncation messaging, and per-video submission behavior remain unchanged.

## Acceptance criteria

1. Existing requests without a limit still show at most 50 entries.
2. A user-entered limit from 1 through 200 requests that many entries.
3. Zero, negative, non-integer, and values above 200 are rejected without invoking `yt-dlp`.
4. The UI keeps the existing playlist URL and reloads the preview on Enter.
5. The server and parser tests cover default, custom, and invalid limits.

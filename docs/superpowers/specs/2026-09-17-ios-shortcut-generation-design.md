# iOS Shortcut Generation Design

## Goal

Let an authenticated Tubemin user generate and download a personalized iOS Shortcut that accepts a shared video URL and submits it to that Tubemin server.

## User flow

1. The user opens Settings while logged in.
2. The user clicks **Generate iOS Shortcut**.
3. Tubemin creates a new API key owned by that user and labels it `iOS Shortcut — <display name>`.
4. Tubemin creates a Shortcut artifact containing the server origin and the newly generated plaintext API key.
5. The browser presents a download link for the artifact.
6. The user opens the downloaded file on iOS and taps **Add Shortcut**.
7. From the iOS Share Sheet, the Shortcut accepts a URL and sends it to `POST /api/submit` with the `X-API-Key` header and JSON body `{ "url": "<shared URL>" }`.

The Shortcut is hosted by Tubemin as a downloadable file; no iCloud Shortcut link is used.

## Server/API design

Add an authenticated generation endpoint under the existing Settings route group:

`POST /settings/shortcut/generate`

The endpoint requires the existing web session and the existing Settings CSRF token. It must:

- determine the authenticated user from `RequireAuth`;
- use the current Settings page origin, supplied by the frontend and validated against the same-origin request, for the Shortcut endpoint URL;
- generate a fresh API key through `api_keys::generate`;
- label the key `iOS Shortcut — <display name>`;
- persist only the bcrypt hash and metadata in the database;
- construct a personalized Shortcut artifact with the plaintext key and server URL;
- return the artifact as a download response.

The plaintext key must not be placed in a query string, redirect URL, HTML page, log message, or database field. If artifact generation fails after key creation, the handler must revoke the newly created key so an unusable credential is not left behind.

The generated key remains visible in the normal API-key listing and can be revoked independently.

## Shortcut artifact

Use a checked-in, minimal Shortcut template/artifact with dedicated template markers for the server URL and API key. Generation replaces only those markers and returns the resulting file with a `.shortcut` filename and an attachment content disposition.

The Shortcut contains only the following behavior:

- accept URL input from the Share Sheet;
- send a JSON `POST` request to `<server-origin>/api/submit`;
- set `X-API-Key` to the generated key;
- show a concise success or error result to the user.

The artifact must be tested on iOS/iPadOS because Apple does not provide a documented server-side API for producing Shortcut files. The implementation should preserve the template's valid file structure and avoid arbitrary user-controlled content in the generated artifact.

## Settings UI

Add a Settings action beside the existing API-key controls:

- button: **Generate iOS Shortcut**;
- after successful generation: show a one-time **Download Shortcut** link;
- explain that each generated Shortcut gets its own API key and can be revoked from the API-key table;
- show an actionable error without exposing the API key or internal error details.

The existing manual API-key generation and revocation flows remain unchanged.

## Error handling

- unauthenticated requests return the existing authentication response;
- invalid CSRF tokens do not generate a key;
- artifact-generation failures return a server error and revoke the just-created key;
- failed downloads must not expose the plaintext key in the response body or logs;
- submitting through the Shortcut continues to use the existing API-key validation and URL validation paths.

## Testing

Add tests for:

- authenticated generation creates an owned key with the exact Shortcut label;
- unauthenticated generation is rejected;
- invalid CSRF tokens do not create keys;
- the generated response has a `.shortcut` download filename and contains the configured endpoint/key in the expected template fields;
- artifact-generation failure revokes the newly created key;
- the generated Shortcut's submission contract uses `POST /api/submit`, `X-API-Key`, and the shared URL JSON field.

Run the existing Rust test suite and a focused artifact-generation test before completion. Real-device installation is a manual acceptance check because Shortcut import validation is performed by iOS.

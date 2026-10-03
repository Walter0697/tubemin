use crate::oidc::RequireAuth;
use crate::state::AppState;
use crate::{api_keys, db, metube};
use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;
use uuid::Uuid;

const MAX_DOWNLOAD_RETRIES: usize = 3;

#[derive(Deserialize)]
pub struct SubtitleTrack {
    pub lang: String,
    pub src: String,
}

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub url: String,
    pub referer: Option<String>,
    pub source_url: Option<String>,
    pub source: Option<String>,
    pub title: Option<String>,
    pub cookies: Option<String>,
    pub subtitle_tracks: Option<Vec<SubtitleTrack>>,
}

#[derive(Serialize)]
pub struct SubmitResponse {
    pub status: String,
}

fn normalize_submitter_tag(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else {
            if last_dash {
                continue;
            }
            last_dash = true;
            '-'
        };
        out.push(mapped);
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "unknown".into()
    } else {
        out.to_string()
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Who is submitting: an API key (extension) or a logged-in web session.
struct Submitter {
    api_key_id: Option<String>,
    owner_sub: Option<String>,
    owner_display: Option<String>,
}

fn source_for_api_key(label: Option<&str>) -> &'static str {
    if label
        .map(str::trim)
        .is_some_and(|label| label.eq_ignore_ascii_case("ios shortcut") || label.to_ascii_lowercase().starts_with("ios shortcut "))
    {
        "shortcut"
    } else {
        "extension"
    }
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    let key = match headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(k) => k.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "missing API key"})),
            )
                .into_response()
        }
    };

    let verified_key = match api_keys::verify_key(&state.pool, &key).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "invalid API key"})),
            )
                .into_response()
        }
        Err(e) => {
            error!(error = %e, "db error verifying API key");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response();
        }
    };

    let _ = api_keys::update_last_used(&state.pool, &verified_key.id).await;
    let submitter = Submitter {
        api_key_id: Some(verified_key.id),
        owner_sub: verified_key.owner_sub,
        owner_display: verified_key.owner_display,
    };
    enqueue(state, body, submitter, source_for_api_key(verified_key.label.as_deref())).await
}

#[derive(Deserialize)]
pub struct WebSubmitRequest {
    pub url: String,
}

/// Dashboard "Add video" dialog: same validation and pipeline as /api/submit,
/// but authenticated by the web session instead of an API key.
pub async fn submit_web(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<WebSubmitRequest>,
) -> impl IntoResponse {
    let submitter = Submitter {
        api_key_id: None,
        owner_sub: user.stable_subject().map(str::to_string),
        owner_display: Some(user.owner_display().to_string()),
    };
    let body = SubmitRequest {
        url: body.url,
        referer: None,
        source_url: None,
        source: None,
        title: None,
        cookies: None,
        subtitle_tracks: None,
    };
    enqueue(state, body, submitter, "web").await
}

async fn enqueue(
    state: AppState,
    mut body: SubmitRequest,
    submitter: Submitter,
    default_source: &str,
) -> Response {
    // Submissions are always single videos. A YouTube URL that names a video is
    // reduced to its canonical watch URL, so playlist parameters never reach
    // MeTube and the stored URL matches what MeTube reports back. Playlist-only
    // URLs go through the dashboard picker instead.
    if let Some(video_id) = crate::url_validator::youtube_video_id(&body.url) {
        body.url = crate::url_validator::canonical_youtube_url(&video_id);
    } else if crate::url_validator::youtube_playlist_id(&body.url).is_some() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "Playlist URLs are not queued directly — add it from the Tubemin dashboard to pick which videos to download"})),
        )
            .into_response();
    }

    let source = normalize_optional_text(body.source.as_deref())
        .or_else(|| Some(default_source.to_string()));

    if !crate::url_validator::is_supported_url(&body.url)
        && !crate::url_validator::is_direct_media_url(&body.url)
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "URL not supported — must be from a site yt-dlp can download"})),
        )
            .into_response();
    }

    // Write the DB row before submitting to MeTube so the watcher always finds
    // a matching row, even when a fast download completes before this handler returns.
    let is_direct = crate::url_validator::is_direct_media_url(&body.url);
    let submitter_tag = submitter
        .owner_display
        .as_deref()
        .map(normalize_submitter_tag);

    // A URL can be submitted from more than one client, but MeTube's queue is
    // global. Reuse the existing active pipeline instead of creating another
    // pending row and downloading the same video twice. Error/interrupted rows
    // intentionally fall through to the existing retry path below.
    if let Ok(Some(existing)) = db::get_active_submission_by_url(&state.pool, &body.url).await {
        tracing::info!(
            url = %body.url,
            submission_id = %existing.id,
            status = %existing.status,
            "submission already exists in active pipeline"
        );
        return (
            StatusCode::OK,
            Json(SubmitResponse {
                status: "queued".into(),
            }),
        )
            .into_response();
    }

    let reused = db::reset_submission_to_pending(
        &state.pool,
        &body.url,
        submitter.api_key_id.as_deref(),
        source.as_deref(),
        submitter.owner_sub.as_deref(),
        submitter.owner_display.as_deref(),
        submitter_tag.as_deref(),
    )
    .await
    .unwrap_or(false);

    // Track the submission ID: use the just-generated one on new submissions to
    // avoid a second DB round-trip; look it up only when reusing an existing row.
    let submission_id: Option<String> = if !reused {
        let id = Uuid::new_v4().to_string();
        if let Err(e) = db::create_submission(
            &state.pool,
            &id,
            &body.url,
            body.source_url.as_deref(),
            source.as_deref(),
            is_direct,
            body.title.as_deref(),
            submitter.api_key_id.as_deref(),
            submitter.owner_sub.as_deref(),
            submitter.owner_display.as_deref(),
            submitter_tag.as_deref(),
        )
        .await
        {
            error!(error = %e, "db error creating submission record");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "db error"})),
            )
                .into_response();
        }
        Some(id)
    } else {
        crate::db::get_submission_by_url(&state.pool, &body.url)
            .await
            .ok()
            .flatten()
            .map(|s| s.id)
    };

    // For direct media URLs (m3u8/mp4) use our own downloader so we can pass
    // the Referer header that many CDNs require.
    if is_direct {
        let url = body.url.clone();
        let referer = body.referer.clone();
        let title = body.title.clone();
        let cookies = body.cookies.clone();
        let subtitle_tracks: Vec<(String, String)> = body
            .subtitle_tracks
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.lang, t.src))
            .collect();
        let pool = state.pool.clone();
        let dl_dir = state.config.downloads_dir.to_string_lossy().to_string();
        let prog_map = state.progress.clone();
        let prog_key = submission_id;

        tokio::spawn(async move {
            let _ = db::mark_downloading(&pool, &url).await;
            if let Some(ref key) = prog_key {
                crate::progress::set(&prog_map, key, 0.0);
            }
            let mut result = None;
            for attempt in 0..=MAX_DOWNLOAD_RETRIES {
                if attempt > 0 {
                    tracing::warn!(
                        url = %url,
                        attempt,
                        "retrying failed direct download"
                    );
                    if let Some(ref key) = prog_key {
                        crate::progress::set(&prog_map, key, 0.0);
                    }
                }
                match crate::direct_download::download(
                    &url,
                    referer.as_deref(),
                    title.as_deref(),
                    cookies.as_deref(),
                    &dl_dir,
                    prog_key.clone(),
                    Some(prog_map.clone()),
                    subtitle_tracks.clone(),
                    Some(&pool),
                )
                .await
                {
                    Ok(filename) => {
                        result = Some(filename);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            url = %url,
                            attempt,
                            "direct download attempt failed"
                        );
                    }
                }
            }
            if let Some(filename) = result {
                let _ = db::mark_imported_by_url(&pool, &url, &filename).await;
            } else {
                tracing::error!(url = %url, "direct download failed after retries");
                let _ = db::mark_active_as_error_by_url(&pool, &url).await;
            }
        });
        return (
            StatusCode::OK,
            Json(SubmitResponse {
                status: "queued".into(),
            }),
        )
            .into_response();
    }

    let mut metube_submitted = false;
    for attempt in 0..=MAX_DOWNLOAD_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        match metube::submit(&state.config.metube_url, &body.url).await {
            Ok(()) => {
                metube_submitted = true;
                break;
            }
            Err(e) => {
                error!(error = %e, attempt, "failed to submit URL to metube");
            }
        }
    }
    if !metube_submitted {
        let _ = db::mark_pending_as_error_by_url(&state.pool, &body.url).await;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "metube unavailable"})),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(SubmitResponse {
            status: "queued".into(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api_keys, config::Config, db, state::AppState};
    use axum::{routing::post, Router};
    use axum_test::TestServer;
    use serde_json::json;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn make_app() -> (TestServer, String, MockServer, Arc<sqlx::SqlitePool>) {
        let pool = Arc::new(db::init("sqlite::memory:").await.unwrap());
        let metube_mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"ok"})))
            .mount(&metube_mock)
            .await;

        let config = Arc::new(Config {
            api_port: 3000,
            metube_url: metube_mock.uri(),
            metube_update_url: None,
            metube_update_token: None,
            metube_auto_update_on_failure: false,
            downloads_dir: "/tmp/downloads".into(),
            peertube_import_dir: "/tmp/import".into(),
            database_url: "sqlite::memory:".into(),
            auth_mode: crate::config::AuthMode::Password,
            admin_password: None,
            cookie_secure: false,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            oidc_redirect_url: None,
            oidc_login_label: "Sign in".into(),
            peertube_url: None,
            peertube_host: None,
            peertube_domain: None,
            peertube_username: None,
            peertube_password: None,
            peertube_admin_email: None,
            peertube_admin_username: None,
            peertube_admin_password: None,
            peertube_video_privacy: 4,
            peertube_oidc_issuer_url: None,
            peertube_oidc_client_id: None,
            peertube_oidc_client_secret: None,
            peertube_cleanup_token: None,
            service_accounts_file: "/tmp/service-accounts.toml".into(),
        });

        let state = AppState {
            pool: pool.clone(),
            config,
            progress: crate::progress::new_progress_map(),
        };
        let api_key = api_keys::generate(
            &pool,
            Some("test"),
            api_keys::ApiKeyOwner {
                sub: Some("test-user"),
                display: "test-user",
            },
        )
        .await
        .unwrap();

        let app = Router::new()
            .route("/api/submit", post(submit))
            .with_state(state);

        (TestServer::new(app).unwrap(), api_key, metube_mock, pool)
    }

    #[tokio::test]
    async fn valid_submission_returns_queued() {
        let (server, api_key, _mock, _pool) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"}))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "queued");
    }

    #[tokio::test]
    async fn duplicate_active_submission_does_not_create_or_resubmit() {
        let (server, api_key, metube_mock, pool) = make_app().await;
        let url = "https://www.youtube.com/watch?v=duplicate-active";

        server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": url}))
            .await
            .assert_status_ok();
        server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": url}))
            .await
            .assert_status_ok();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE url = ?")
            .bind(url)
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            metube_mock
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.url.path() == "/add")
                .count(),
            1
        );
    }

    fn metube_add_urls(requests: &[wiremock::Request]) -> Vec<String> {
        requests
            .iter()
            .filter(|request| request.url.path() == "/add")
            .map(|request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                body["url"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[tokio::test]
    async fn playlist_only_url_is_rejected_without_submitting() {
        let (server, api_key, metube_mock, pool) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": "https://www.youtube.com/playlist?list=PL123"}))
            .await;
        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(metube_add_urls(&metube_mock.received_requests().await.unwrap()).is_empty());
    }

    #[tokio::test]
    async fn video_in_playlist_is_submitted_as_single_canonical_video() {
        let (server, api_key, metube_mock, pool) = make_app().await;
        server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": "https://youtube.com/watch?v=dQw4w9WgXcQ&list=RDdQw4w9WgXcQ&start_radio=1"}))
            .await
            .assert_status_ok();

        let canonical = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let (url, key): (String, Option<String>) =
            sqlx::query_as("SELECT url, video_key FROM submissions")
                .fetch_one(pool.as_ref())
                .await
                .unwrap();
        assert_eq!(url, canonical);
        assert_eq!(key.as_deref(), Some("youtube:dQw4w9WgXcQ"));
        assert_eq!(
            metube_add_urls(&metube_mock.received_requests().await.unwrap()),
            vec![canonical.to_string()]
        );
    }

    #[tokio::test]
    async fn duplicate_is_detected_across_youtube_url_variants() {
        let (server, api_key, metube_mock, pool) = make_app().await;
        // A row stored before URLs were canonicalised still matches by key.
        db::create_submission(
            &pool,
            "legacy",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=30",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        for url in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123&index=4",
        ] {
            server
                .post("/api/submit")
                .add_header("X-API-Key", &api_key)
                .json(&json!({"url": url}))
                .await
                .assert_status_ok();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM submissions")
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(metube_add_urls(&metube_mock.received_requests().await.unwrap()).is_empty());
    }

    #[tokio::test]
    async fn shortcut_api_key_records_shortcut_source() {
        let (server, _api_key, _mock, pool) = make_app().await;
        let shortcut_key = api_keys::generate(
            &pool,
            Some("iOS Shortcut"),
            api_keys::ApiKeyOwner {
                sub: Some("test-user"),
                display: "test-user",
            },
        )
        .await
        .unwrap();
        let url = "https://www.youtube.com/watch?v=shortcut-source-test";
        let response = server
            .post("/api/submit")
            .add_header("X-API-Key", &shortcut_key)
            .json(&json!({"url": url}))
            .await;
        response.assert_status_ok();
        let submission = db::get_submission_by_url(&pool, url)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submission.source.as_deref(), Some("shortcut"));
    }

    #[test]
    fn ios_shortcut_keys_use_shortcut_source() {
        assert_eq!(source_for_api_key(Some("iOS Shortcut")), "shortcut");
        assert_eq!(source_for_api_key(Some("iOS Shortcut — Walter")), "shortcut");
        assert_eq!(source_for_api_key(Some("browser extension")), "extension");
        assert_eq!(source_for_api_key(None), "extension");
    }

    #[tokio::test]
    async fn missing_source_defaults_to_extension() {
        let (server, api_key, _mock, pool) = make_app().await;
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": url}))
            .await;
        resp.assert_status_ok();
        let submission = db::get_submission_by_url(&pool, url)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submission.source.as_deref(), Some("extension"));
    }

    async fn make_web_app() -> (TestServer, MockServer, Arc<sqlx::SqlitePool>) {
        use crate::oidc::{OidcUser, SESSION_USER_KEY};
        use axum::routing::get;
        use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

        let pool = Arc::new(db::init("sqlite::memory:").await.unwrap());
        let metube_mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"ok"})))
            .mount(&metube_mock)
            .await;

        let config = Arc::new(Config {
            api_port: 3000,
            metube_url: metube_mock.uri(),
            metube_update_url: None,
            metube_update_token: None,
            metube_auto_update_on_failure: false,
            downloads_dir: "/tmp/downloads".into(),
            peertube_import_dir: "/tmp/import".into(),
            database_url: "sqlite::memory:".into(),
            auth_mode: crate::config::AuthMode::Password,
            admin_password: None,
            cookie_secure: false,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            oidc_redirect_url: None,
            oidc_login_label: "Sign in".into(),
            peertube_url: None,
            peertube_host: None,
            peertube_domain: None,
            peertube_username: None,
            peertube_password: None,
            peertube_admin_email: None,
            peertube_admin_username: None,
            peertube_admin_password: None,
            peertube_video_privacy: 4,
            peertube_oidc_issuer_url: None,
            peertube_oidc_client_id: None,
            peertube_oidc_client_secret: None,
            peertube_cleanup_token: None,
            service_accounts_file: "/tmp/service-accounts.toml".into(),
        });

        let state = AppState {
            pool: pool.clone(),
            config,
            progress: crate::progress::new_progress_map(),
        };

        async fn test_login(session: Session) -> &'static str {
            session
                .insert(
                    SESSION_USER_KEY,
                    OidcUser {
                        sub: Some("web-user".into()),
                        email: "walter@example.com".into(),
                        username: Some("walter".into()),
                    },
                )
                .await
                .unwrap();
            "ok"
        }

        let app = Router::new()
            .route("/api/submissions/create", post(submit_web))
            .route("/test/login", get(test_login))
            .layer(SessionManagerLayer::new(MemoryStore::default()))
            .with_state(state);

        let mut server = TestServer::new(app).unwrap();
        server.do_save_cookies();
        (server, metube_mock, pool)
    }

    #[tokio::test]
    async fn web_submission_requires_session() {
        let (server, _mock, _pool) = make_web_app().await;
        let resp = server
            .post("/api/submissions/create")
            .json(&json!({"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"}))
            .await;
        resp.assert_status(StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn web_submission_creates_with_web_source() {
        let (server, _mock, pool) = make_web_app().await;
        server.get("/test/login").await.assert_status_ok();

        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let resp = server
            .post("/api/submissions/create")
            .json(&json!({"url": url}))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "queued");

        let submission = db::get_submission_by_url(&pool, url)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submission.source.as_deref(), Some("web"));
        assert_eq!(submission.submitter_display.as_deref(), Some("walter"));
    }

    #[tokio::test]
    async fn web_submission_rejects_unsupported_url() {
        let (server, _mock, _pool) = make_web_app().await;
        server.get("/test/login").await.assert_status_ok();

        let resp = server
            .post("/api/submissions/create")
            .json(&json!({"url": "https://randomsite.xyz/page"}))
            .await;
        resp.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn missing_api_key_returns_401() {
        let (server, _, _mock, _) = make_app().await;
        let resp = server
            .post("/api/submit")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_api_key_returns_401() {
        let (server, _, _mock, _) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", "wrong-key")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }
}

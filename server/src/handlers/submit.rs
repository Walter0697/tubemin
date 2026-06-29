use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;
use uuid::Uuid;
use crate::state::AppState;
use crate::{api_keys, db, metube};

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub url: String,
    pub referer: Option<String>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub cookies: Option<String>,
}

#[derive(Serialize)]
pub struct SubmitResponse {
    pub status: String,
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    let key = match headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(k) => k.to_string(),
        None => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing API key"}))).into_response(),
    };

    let key_id = match api_keys::verify_key(&state.pool, &key).await {
        Ok(Some(id)) => id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid API key"}))).into_response(),
        Err(e) => {
            error!(error = %e, "db error verifying API key");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response();
        }
    };

    let _ = api_keys::update_last_used(&state.pool, &key_id).await;

    if !crate::url_validator::is_supported_url(&body.url)
        && !crate::url_validator::is_direct_media_url(&body.url)
    {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": "URL not supported — must be from a site yt-dlp can download"}))).into_response();
    }

    // Write the DB row before submitting to MeTube so the watcher always finds
    // a matching row, even when a fast download completes before this handler returns.
    let is_direct = crate::url_validator::is_direct_media_url(&body.url);
    let reused = db::reset_submission_to_pending(&state.pool, &body.url).await.unwrap_or(false);
    if !reused {
        let id = Uuid::new_v4().to_string();
        if let Err(e) = db::create_submission(&state.pool, &id, &body.url, body.source_url.as_deref(), is_direct, body.title.as_deref()).await {
            error!(error = %e, "db error creating submission record");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response();
        }
    }

    // For direct media URLs (m3u8/mp4) use our own downloader so we can pass
    // the Referer header that many CDNs require.
    if is_direct {
        let url       = body.url.clone();
        let referer   = body.referer.clone();
        let title     = body.title.clone();
        let cookies   = body.cookies.clone();
        let pool      = state.pool.clone();
        let dl_dir    = state.config.downloads_dir.to_string_lossy().to_string();
        let prog_map  = state.progress.clone();

        let prog_key: Option<String> = if reused {
            crate::db::get_submission_by_url(&pool, &url).await
                .ok().flatten().map(|s| s.id)
        } else {
            crate::db::get_submission_by_url(&pool, &url).await
                .ok().flatten().map(|s| s.id)
        };

        tokio::spawn(async move {
            let _ = db::mark_downloading(&pool, &url).await;
            if let Some(ref key) = prog_key {
                crate::progress::set(&prog_map, key, 0.0);
            }
            match crate::direct_download::download(
                &url,
                referer.as_deref(),
                title.as_deref(),
                cookies.as_deref(),
                &dl_dir,
                prog_key,
                Some(prog_map),
            ).await {
                Ok(filename) => {
                    let _ = db::mark_imported_by_url(&pool, &url, &filename).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, url = %url, "direct download failed");
                    let _ = db::mark_pending_as_error_by_url(&pool, &url).await;
                }
            }
        });
        return (StatusCode::OK, Json(SubmitResponse { status: "queued".into() })).into_response();
    }

    if let Err(e) = metube::submit(&state.config.metube_url, &body.url).await {
        error!(error = %e, "failed to submit URL to metube");
        let _ = db::mark_pending_as_error_by_url(&state.pool, &body.url).await;
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "metube unavailable"}))).into_response();
    }

    (StatusCode::OK, Json(SubmitResponse { status: "queued".into() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use axum_test::TestServer;
    use std::sync::Arc;
    use crate::{config::Config, db, api_keys, state::AppState};
    use serde_json::json;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    async fn make_app() -> (TestServer, String, MockServer) {
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
            downloads_dir: "/tmp/downloads".into(),
            peertube_import_dir: "/tmp/import".into(),
            database_url: "sqlite::memory:".into(),
            auth_mode: crate::config::AuthMode::Password,
            admin_password: None,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            oidc_redirect_url: None,
            peertube_url: None,
            peertube_host: None,
            peertube_username: None,
            peertube_password: None,
            peertube_admin_email: None,
            peertube_admin_username: None,
            peertube_admin_password: None,
        });

        let state = AppState { pool: pool.clone(), config, progress: crate::progress::new_progress_map() };
        let api_key = api_keys::generate(&pool, Some("test")).await.unwrap();

        let app = Router::new()
            .route("/api/submit", post(submit))
            .with_state(state);

        (TestServer::new(app).unwrap(), api_key, metube_mock)
    }

    #[tokio::test]
    async fn valid_submission_returns_queued() {
        let (server, api_key, _mock) = make_app().await;
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
    async fn missing_api_key_returns_401() {
        let (server, _, _mock) = make_app().await;
        let resp = server
            .post("/api/submit")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_api_key_returns_401() {
        let (server, _, _mock) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", "wrong-key")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }
}

use crate::{db, oidc::RequireAuth, peertube, state::AppState};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static CLEANUP_ENV: OnceLock<Environment<'static>> = OnceLock::new();

fn cleanup_env() -> &'static Environment<'static> {
    CLEANUP_ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        env.add_template("nav", include_str!("../../templates/partials/nav.html"))
            .unwrap();
        env.add_template("cleanup", include_str!("../../templates/cleanup.html"))
            .unwrap();
        env
    })
}

fn peertube_configured(state: &AppState) -> bool {
    state.config.peertube_url.is_some()
        && state.config.peertube_username.is_some()
        && state.config.peertube_password.is_some()
}

fn peertube_base(state: &AppState, req: &Request) -> String {
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    state
        .config
        .peertube_host
        .as_ref()
        .map(|h| format!("{}://{}", scheme, h))
        .unwrap_or_default()
}

pub async fn cleanup_page(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    req: Request,
) -> Html<String> {
    let tmpl = cleanup_env().get_template("cleanup").unwrap();
    let ctx = minijinja::context! {
        peertube_base => peertube_base(&state, &req),
        username => user.display_name(),
        active_page => "cleanup",
        app_version => env!("CARGO_PKG_VERSION"),
        asset_version => crate::ASSET_VERSION,
        peertube_enabled => peertube_configured(&state),
        submitter_tag => user.submitter_tag(),
    };
    Html(
        tmpl.render(ctx)
            .unwrap_or_else(|e| format!("Template error: {}", e)),
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanVideoRow {
    pub uuid: String,
    pub title: String,
    pub thumbnail_path: Option<String>,
    pub preview_path: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Serialize)]
pub struct OrphanListResponse {
    pub videos: Vec<OrphanVideoRow>,
    pub total: usize,
}

pub async fn list_orphan_videos(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let (Some(pt_url), Some(pt_user), Some(pt_pass)) = (
        &state.config.peertube_url,
        &state.config.peertube_username,
        &state.config.peertube_password,
    ) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "PeerTube is not configured"})),
        )
            .into_response();
    };

    let known_uuids = match db::all_peertube_uuids(&state.pool).await {
        Ok(uuids) => uuids,
        Err(e) => {
            tracing::error!("DB error loading PeerTube UUIDs: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "db error"})),
            )
                .into_response();
        }
    };

    let submitter_tag = user.submitter_tag();
    let Some(tag_filter) = peertube::submitter_peer_tube_tag(&submitter_tag) else {
        return Json(OrphanListResponse {
            videos: vec![],
            total: 0,
        })
        .into_response();
    };
    let videos = match peertube::list_account_videos(
        pt_url,
        state.config.peertube_host.as_deref(),
        pt_user,
        pt_pass,
        Some(&tag_filter),
    )
    .await
    {
        Ok(videos) => videos,
        Err(e) => {
            tracing::error!("PeerTube list error: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "could not list PeerTube videos"})),
            )
                .into_response();
        }
    };

    let orphans: Vec<OrphanVideoRow> = videos
        .into_iter()
        .filter(|video| !known_uuids.contains(&video.uuid))
        .map(|video| OrphanVideoRow {
            uuid: video.uuid,
            title: video.title,
            thumbnail_path: video.thumbnail_path,
            preview_path: video.preview_path,
            published_at: video.published_at,
        })
        .collect();

    let total = orphans.len();
    Json(OrphanListResponse {
        videos: orphans,
        total,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct DeleteOrphansRequest {
    pub uuids: Vec<String>,
}

#[derive(Serialize)]
pub struct DeleteOrphansResponse {
    pub deleted: usize,
    pub failed: usize,
}

pub async fn delete_orphan_videos(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<DeleteOrphansRequest>,
) -> impl IntoResponse {
    if body.uuids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no uuids provided"})),
        )
            .into_response();
    }

    let (Some(pt_url), Some(pt_user), Some(pt_pass)) = (
        &state.config.peertube_url,
        &state.config.peertube_username,
        &state.config.peertube_password,
    ) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "PeerTube is not configured"})),
        )
            .into_response();
    };

    let submitter_tag = user.submitter_tag();
    let mut deleted = 0usize;
    let mut failed = 0usize;

    for uuid in body.uuids {
        let tags = match peertube::get_video_tags(
            pt_url,
            state.config.peertube_host.as_deref(),
            pt_user,
            pt_pass,
            &uuid,
        )
        .await
        {
            Ok(tags) => tags,
            Err(e) => {
                tracing::warn!("Could not load tags for orphan {}: {}", uuid, e);
                failed += 1;
                continue;
            }
        };

        if !peertube::video_owned_by_submitter(&tags, &submitter_tag) {
            tracing::warn!(
                "Refusing orphan delete for {}: submitter tag mismatch",
                uuid
            );
            failed += 1;
            continue;
        }

        if let Err(e) = peertube::delete_video(
            pt_url,
            state.config.peertube_host.as_deref(),
            pt_user,
            pt_pass,
            &uuid,
        )
        .await
        {
            tracing::warn!("PeerTube delete failed for orphan {}: {}", uuid, e);
            failed += 1;
        } else {
            deleted += 1;
        }
    }

    Json(DeleteOrphansResponse { deleted, failed }).into_response()
}

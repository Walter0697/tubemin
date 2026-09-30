use crate::{
    db::{self, Submission},
    peertube::{self, ListedVideo},
    service_accounts::{RequireServiceAccount, ServiceScope},
    source_cleanup,
    state::AppState,
};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::{HashMap, HashSet}, path::{Path as FsPath, PathBuf}};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCatalogItem {
    pub id: String,
    pub title: String,
    pub thumbnail_url: String,
    pub published_at: Option<String>,
    pub processing_state: String,
    pub original_media_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceVideoDetail {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub duration: Option<u64>,
    pub published_at: Option<String>,
    pub thumbnail_url: String,
    pub original_media_available: bool,
}

#[derive(Debug, Deserialize)]
pub struct CompletionRequest {
    pub destination: String,
    pub destination_ref: Option<String>,
    pub output_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct FailureRequest {
    pub error: String,
    pub destination: Option<String>,
    pub destination_ref: Option<String>,
}

fn service_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn peertube_credentials(state: &AppState) -> Result<(&str, Option<&str>, &str, &str), Response> {
    match (
        state.config.peertube_url.as_deref(),
        state.config.peertube_host.as_deref(),
        state.config.peertube_username.as_deref(),
        state.config.peertube_password.as_deref(),
    ) {
        (Some(url), host, Some(username), Some(password)) => Ok((url, host, username, password)),
        _ => Err(service_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "PeerTube service account is not configured",
        )),
    }
}

fn filter_owned_videos(
    listed: Vec<ListedVideo>,
    known_uuids: &HashSet<String>,
) -> Vec<ListedVideo> {
    listed
        .into_iter()
        .filter(|video| known_uuids.contains(&video.uuid))
        .collect()
}

fn public_thumbnail_url(scheme: &str, host: &str, path: &str) -> String {
    format!(
        "{}://{}{}",
        scheme,
        host,
        if path.starts_with('/') { path.to_string() } else { format!("/{path}") }
    )
}

fn safe_media_filename(filename: &str) -> bool {
    source_cleanup::is_safe_filename(filename)
}

fn local_source_path(state: &AppState, filename: Option<&str>) -> Option<PathBuf> {
    let filename = filename.filter(|name| safe_media_filename(name))?;
    [
        state.config.downloads_dir.clone(),
        state.config.peertube_import_dir.clone(),
    ]
    .into_iter()
    .map(|root| root.join(filename))
    .find(|path| path.is_file())
}

fn local_mime(path: &FsPath) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn submission_map(submissions: Vec<Submission>) -> HashMap<String, Submission> {
    submissions
        .into_iter()
        .filter_map(|submission| submission.peertube_uuid.clone().map(|uuid| (uuid, submission)))
        .collect()
}

async fn ensure_owned_video(
    state: &AppState,
    uuid: &str,
) -> Result<(), Response> {
    let (url, host, username, password) = peertube_credentials(state)?;
    let videos = peertube::list_account_videos(url, host, username, password, None)
        .await
        .map_err(|error| {
            tracing::warn!(error = %error, uuid = %uuid, "service ownership lookup failed");
            service_error(StatusCode::BAD_GATEWAY, "PeerTube ownership lookup failed")
        })?;
    if !videos.iter().any(|video| video.uuid == uuid) {
        return Err(service_error(StatusCode::NOT_FOUND, "video not found"));
    }
    Ok(())
}

pub async fn service_catalog(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    request: Request,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::CatalogRead)
    {
        return response;
    }
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    let Some(public_host) = host else {
        return service_error(StatusCode::SERVICE_UNAVAILABLE, "PeerTube public host is not configured");
    };
    let scheme = request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let submissions = match db::active_catalog_submissions(&state.pool).await {
        Ok(submissions) => submissions,
        Err(error) => {
            tracing::error!(error = %error, "service catalog database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "catalog lookup failed");
        }
    };
    let by_uuid = submission_map(submissions);
    let listed = match peertube::list_account_videos(url, host, username, password, None).await {
        Ok(videos) => videos,
        Err(error) => {
            tracing::error!(error = %error, "service catalog PeerTube lookup failed");
            return service_error(StatusCode::BAD_GATEWAY, "PeerTube catalog lookup failed");
        }
    };
    let known = by_uuid.keys().cloned().collect::<HashSet<_>>();
    let videos = filter_owned_videos(listed, &known)
        .into_iter()
        .filter_map(|video| {
            let submission = by_uuid.get(&video.uuid)?;
            Some(ServiceCatalogItem {
                id: video.uuid.clone(),
                title: submission.title.clone().unwrap_or(video.title),
                thumbnail_url: video
                    .thumbnail_path
                    .as_deref()
                    .or(video.preview_path.as_deref())
                    .map(|path| public_thumbnail_url(scheme, public_host, path))
                    .unwrap_or_default(),
                published_at: video.published_at,
                processing_state: submission.status.clone(),
                original_media_available: true,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "videos": videos })).into_response()
}

pub async fn service_video(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::CatalogRead)
    {
        return response;
    }
    let submissions = match db::active_catalog_submissions(&state.pool).await {
        Ok(submissions) => submissions,
        Err(error) => {
            tracing::error!(error = %error, "service video database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "video lookup failed");
        }
    };
    let Some(submission) = submissions.into_iter().find(|item| item.peertube_uuid.as_deref() == Some(uuid.as_str())) else {
        return service_error(StatusCode::NOT_FOUND, "video not found");
    };
    if let Err(response) = ensure_owned_video(&state, &uuid).await {
        return response;
    }
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    let Some(public_host) = host else {
        return service_error(StatusCode::SERVICE_UNAVAILABLE, "PeerTube public host is not configured");
    };
    let scheme = request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let detail = match peertube::get_video(url, host, username, password, &uuid).await {
        Ok(detail) => detail,
        Err(error) => {
            tracing::warn!(error = %error, uuid = %uuid, "service video PeerTube lookup failed");
            return service_error(StatusCode::BAD_GATEWAY, "PeerTube video lookup failed");
        }
    };
    let original_media_available = detail.best_download_url().is_some() || submission.filename.is_some();
    Json(ServiceVideoDetail {
        id: uuid.clone(),
        title: submission.title.unwrap_or(detail.name),
        description: detail.description,
        duration: detail.duration,
        published_at: detail.published_at,
        thumbnail_url: detail
            .thumbnail_path
            .as_deref()
            .map(|path| public_thumbnail_url(scheme, public_host, path))
            .unwrap_or_default(),
        original_media_available,
    })
    .into_response()
}

pub async fn service_media(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::MediaRead)
    {
        return response;
    }
    let submissions = match db::active_catalog_submissions(&state.pool).await {
        Ok(submissions) => submissions,
        Err(error) => {
            tracing::error!(error = %error, "service media database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "media lookup failed");
        }
    };
    let Some(submission) = submissions.into_iter().find(|item| item.peertube_uuid.as_deref() == Some(uuid.as_str())) else {
        return service_error(StatusCode::NOT_FOUND, "video not found");
    };
    if let Err(response) = ensure_owned_video(&state, &uuid).await {
        return response;
    }
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    if let Some(path) = local_source_path(&state, submission.filename.as_deref()) {
        let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or("video");
        let file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(error = %error, path = %path.display(), "local service media open failed");
                return service_error(StatusCode::NOT_FOUND, "source media not found");
            }
        };
        let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
        response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(local_mime(&path)));
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename)) {
            response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
        }
        return response;
    }
    let upstream = match peertube::stream_original(url, host, username, password, &uuid).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(error = %error, uuid = %uuid, "PeerTube source media request failed");
            return service_error(StatusCode::BAD_GATEWAY, "source media unavailable");
        }
    };
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let content_length = upstream.headers().get(header::CONTENT_LENGTH).cloned();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    if let Some(value) = content_type {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if let Some(value) = content_length {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

pub async fn service_thumbnail(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::MediaRead)
    {
        return response;
    }
    let owned = db::active_catalog_submissions(&state.pool)
        .await
        .map(|rows| rows.into_iter().any(|row| row.peertube_uuid.as_deref() == Some(uuid.as_str())));
    match owned {
        Ok(true) => {}
        Ok(false) => return service_error(StatusCode::NOT_FOUND, "video not found"),
        Err(error) => {
            tracing::error!(error = %error, "service thumbnail database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "thumbnail lookup failed");
        }
    }
    if let Err(response) = ensure_owned_video(&state, &uuid).await {
        return response;
    }
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    match peertube::fetch_thumbnail(url, host, username, password, &uuid).await {
        Ok((content_type, bytes)) => {
            let mut response = Response::new(Body::from(bytes));
            if let Ok(value) = HeaderValue::from_str(&content_type) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            response
        }
        Err(error) => {
            tracing::warn!(error = %error, uuid = %uuid, "PeerTube thumbnail request failed");
            service_error(StatusCode::BAD_GATEWAY, "thumbnail unavailable")
        }
    }
}

async fn remove_local_sources(state: &AppState, filename: Option<&str>) -> Result<Vec<String>, String> {
    let Some(filename) = filename else {
        return Ok(Vec::new());
    };
    if !safe_media_filename(filename) {
        return Err("submission filename is unsafe".into());
    }
    let _upload_guard = crate::watcher::upload_lock().lock().await;
    let mut removed = Vec::new();
    for root in [
        state.config.peertube_import_dir.clone(),
        state.config.downloads_dir.clone(),
    ] {
        for path in source_cleanup::source_artifacts(&root, filename) {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => removed.push(path.display().to_string()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("could not remove {}: {error}", path.display())),
            }
        }
    }
    Ok(removed)
}

pub async fn service_complete(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    Json(request): Json<CompletionRequest>,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::TransferComplete)
    {
        return response;
    }
    if request.destination.trim().is_empty() {
        return service_error(StatusCode::BAD_REQUEST, "destination is required");
    }
    let submission = match db::find_submission_by_peertube_uuid(&state.pool, &uuid).await {
        Ok(Some(submission)) => submission,
        Ok(None) => return service_error(StatusCode::NOT_FOUND, "video not found"),
        Err(error) => {
            tracing::error!(error = %error, "service completion database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "completion lookup failed");
        }
    };
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
    let owned = match peertube::list_account_videos(url, host, username, password, None).await {
        Ok(videos) => videos.iter().any(|video| video.uuid == uuid),
        Err(error) => {
            tracing::warn!(error = %error, uuid = %uuid, "service completion ownership lookup failed");
            return service_error(StatusCode::BAD_GATEWAY, "PeerTube ownership lookup failed");
        }
    };
    if !owned {
        return service_error(StatusCode::NOT_FOUND, "video not found");
    }
    let existing = match db::get_transfer_for_video(&state.pool, &uuid, &principal.id).await {
        Ok(existing) => existing,
        Err(error) => {
            tracing::error!(error = %error, "service completion transfer lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "completion lookup failed");
        }
    };
    if let Some(transfer) = existing.as_ref() {
        if transfer.state == "deleted" {
            return Json(json!({ "status": "deleted", "transfer_id": transfer.id, "already_deleted": true })).into_response();
        }
    }
    let transfer_id = existing
        .as_ref()
        .map(|transfer| transfer.id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if existing.is_none() {
        let now = chrono::Utc::now().to_rfc3339();
        let transfer = db::TransferRow {
            id: transfer_id.clone(),
            submission_id: submission.id.clone(),
            peertube_uuid: uuid.clone(),
            service_account_id: principal.id.clone(),
            consumer: principal.name.clone(),
            destination: request.destination.clone(),
            destination_ref: request.destination_ref.clone(),
            source_title: submission.title.clone(),
            source_url: Some(submission.url.clone()),
            state: "processing".into(),
            output_size: request.output_size,
            source_cleanup_state: "pending".into(),
            peertube_delete_state: "pending".into(),
            error: None,
            retry_count: 0,
            created_at: now.clone(),
            completed_at: None,
            deleted_at: None,
            updated_at: now,
        };
        if let Err(error) = db::create_transfer(&state.pool, &transfer).await {
            tracing::error!(error = %error, "service completion transfer creation failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "could not create transfer");
        }
    }
    if let Err(error) = db::set_submission_status(&state.pool, &uuid, "processing").await {
        tracing::error!(error = %error, "service completion state transition failed");
        return service_error(StatusCode::INTERNAL_SERVER_ERROR, "could not update video state");
    }
    let source_error = remove_local_sources(&state, submission.filename.as_deref())
        .await
        .err();
    let source_state = if source_error.is_some() { "error" } else { "complete" };
    let _ = db::set_transfer_state(
        &state.pool,
        &transfer_id,
        "cleanup_pending",
        source_state,
        "pending",
        source_error.as_deref(),
        Some(&chrono::Utc::now().to_rfc3339()),
        None,
    )
    .await;

    if let Err(error) = peertube::delete_video(url, host, username, password, &uuid).await {
        let message = error.to_string();
        let _ = db::set_transfer_state(
            &state.pool,
            &transfer_id,
            "cleanup_pending",
            source_state,
            "error",
            Some(&message),
            Some(&chrono::Utc::now().to_rfc3339()),
            None,
        )
        .await;
        let _ = db::set_submission_status(&state.pool, &uuid, "cleanup_pending").await;
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "status": "cleanup_pending", "transfer_id": transfer_id, "error": message })),
        )
            .into_response();
    }

    let deleted_at = chrono::Utc::now().to_rfc3339();
    let _ = db::set_transfer_state(
        &state.pool,
        &transfer_id,
        "deleted",
        source_state,
        "complete",
        source_error.as_deref(),
        Some(&deleted_at),
        Some(&deleted_at),
    )
    .await;
    if let Err(error) = db::set_submission_status(&state.pool, &uuid, "deleted").await {
        tracing::error!(error = %error, uuid = %uuid, "service completion final state update failed");
    }
    Json(json!({
        "status": "deleted",
        "transfer_id": transfer_id,
        "source_cleanup": source_state,
    }))
    .into_response()
}

pub async fn service_fail(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    Json(request): Json<FailureRequest>,
) -> Response {
    if let Err(response) = (RequireServiceAccount { principal: principal.clone() })
        .require(ServiceScope::TransferFail)
    {
        return response;
    }
    if request.error.trim().is_empty() {
        return service_error(StatusCode::BAD_REQUEST, "error is required");
    }
    let submission = match db::find_submission_by_peertube_uuid(&state.pool, &uuid).await {
        Ok(Some(submission)) => submission,
        Ok(None) => return service_error(StatusCode::NOT_FOUND, "video not found"),
        Err(error) => {
            tracing::error!(error = %error, "service failure database lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "failure lookup failed");
        }
    };
    let existing = match db::get_transfer_for_video(&state.pool, &uuid, &principal.id).await {
        Ok(existing) => existing,
        Err(error) => {
            tracing::error!(error = %error, "service failure transfer lookup failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "failure lookup failed");
        }
    };
    let transfer_id = existing
        .as_ref()
        .map(|transfer| transfer.id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if existing.is_none() {
        let now = chrono::Utc::now().to_rfc3339();
        let transfer = db::TransferRow {
            id: transfer_id.clone(),
            submission_id: submission.id,
            peertube_uuid: uuid.clone(),
            service_account_id: principal.id,
            consumer: principal.name,
            destination: request.destination.unwrap_or_else(|| "unknown".into()),
            destination_ref: request.destination_ref,
            source_title: submission.title,
            source_url: Some(submission.url),
            state: "failed".into(),
            output_size: None,
            source_cleanup_state: "pending".into(),
            peertube_delete_state: "pending".into(),
            error: Some(request.error.clone()),
            retry_count: 1,
            created_at: now.clone(),
            completed_at: None,
            deleted_at: None,
            updated_at: now,
        };
        if let Err(error) = db::create_transfer(&state.pool, &transfer).await {
            tracing::error!(error = %error, "service failure transfer creation failed");
            return service_error(StatusCode::INTERNAL_SERVER_ERROR, "could not record failure");
        }
    } else {
        let _ = db::update_transfer_cleanup(&state.pool, &transfer_id, "failed", Some(&request.error)).await;
    }
    let _ = db::set_submission_status(&state.pool, &uuid, "error").await;
    Json(json!({ "status": "failed", "transfer_id": transfer_id })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_items_only_include_peer_tube_items_with_known_submissions() {
        let listed = vec![
            ListedVideo {
                uuid: "known".into(),
                title: "Known".into(),
                thumbnail_path: None,
                preview_path: None,
                published_at: None,
                tags: vec![],
            },
            ListedVideo {
                uuid: "unknown".into(),
                title: "Unknown".into(),
                thumbnail_path: None,
                preview_path: None,
                published_at: None,
                tags: vec![],
            },
        ];
        let known = ["known".to_string()].into_iter().collect();

        let result = filter_owned_videos(listed, &known);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uuid, "known");
    }

    #[test]
    fn public_thumbnail_url_uses_peer_tube_host_without_exposing_credentials() {
        assert_eq!(
            public_thumbnail_url("https", "videos.example.com", "/lazy-static/thumb.jpg"),
            "https://videos.example.com/lazy-static/thumb.jpg"
        );
    }

    #[test]
    fn media_filename_rejects_path_traversal() {
        assert!(safe_media_filename("video.mp4"));
        assert!(!safe_media_filename("../video.mp4"));
        assert!(!safe_media_filename("/tmp/video.mp4"));
    }
}

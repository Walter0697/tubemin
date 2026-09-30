use crate::{
    db::{self, Submission},
    peertube::{self, ListedVideo},
    service_accounts::{RequireServiceAccount, ServiceScope},
    source_cleanup,
    state::AppState,
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;
use std::{collections::{HashMap, HashSet}, path::{Path as FsPath, PathBuf}};
use tokio_util::io::ReaderStream;

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

pub async fn service_catalog(
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
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
                thumbnail_url: format!("/api/service/videos/{}/thumbnail", video.uuid),
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
        thumbnail_url: format!("/api/service/videos/{}/thumbnail", uuid),
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
    let (url, host, username, password) = match peertube_credentials(&state) {
        Ok(credentials) => credentials,
        Err(response) => return response,
    };
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
    fn media_filename_rejects_path_traversal() {
        assert!(safe_media_filename("video.mp4"));
        assert!(!safe_media_filename("../video.mp4"));
        assert!(!safe_media_filename("/tmp/video.mp4"));
    }
}

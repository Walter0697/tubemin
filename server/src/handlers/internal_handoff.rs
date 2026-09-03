use crate::{db, state::AppState, watcher};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct HandoffRequest {
    pub items: Vec<HandoffRequestItem>,
}

#[derive(Debug, Deserialize)]
pub struct HandoffRequestItem {
    pub peertube_uuid: String,
    pub filename: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HandoffResponse {
    pub checked: usize,
    pub claimed: usize,
    pub removed_files: Vec<String>,
    pub already_claimed: usize,
    pub not_found: usize,
}

pub async fn handoff(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<HandoffRequest>,
) -> impl IntoResponse {
    let Some(expected) = state.config.peertube_cleanup_token.as_deref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "handoff is disabled"})),
        )
            .into_response();
    };
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied != Some(expected) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid handoff token"})),
        )
            .into_response();
    }
    if request.items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "items must not be empty"})),
        )
            .into_response();
    }

    let _upload_guard = watcher::upload_lock().lock().await;
    let mut response = HandoffResponse {
        checked: request.items.len(),
        claimed: 0,
        removed_files: Vec::new(),
        already_claimed: 0,
        not_found: 0,
    };

    for item in request.items {
        if item.filename.as_deref().is_some_and(|filename| !is_safe_filename(filename)) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "filename must be a plain file name"})),
            )
                .into_response();
        }

        let result = match db::claim_handoff_item(
            &state.pool,
            &db::HandoffItem {
                peertube_uuid: Some(item.peertube_uuid),
                filename: item.filename.clone(),
            },
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(error = %error, filename = ?item.filename, "handoff database claim failed");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "handoff database claim failed"})),
                )
                    .into_response();
            }
        };

        match result.state {
            db::HandoffItemState::Claimed => response.claimed += 1,
            db::HandoffItemState::AlreadyClaimed => response.already_claimed += 1,
            db::HandoffItemState::NotFound => {
                response.not_found += 1;
                continue;
            }
        }

        if let Some(filename) = item.filename.as_deref() {
            let roots = [
                state.config.peertube_import_dir.clone(),
                state.config.downloads_dir.clone(),
            ];
            for root in roots {
                for path in source_artifacts(&root, filename) {
                    match tokio::fs::remove_file(&path).await {
                        Ok(()) => response.removed_files.push(path.display().to_string()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            tracing::error!(error = %error, path = %path.display(), "handoff source cleanup failed");
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": "handoff source cleanup failed"})),
                            )
                                .into_response();
                        }
                    }
                }
            }
        }
    }

    Json(response).into_response()
}

fn is_safe_filename(filename: &str) -> bool {
    let path = Path::new(filename);
    !filename.is_empty() && path.file_name().and_then(|name| name.to_str()) == Some(filename)
}

fn source_artifacts(root: &Path, filename: &str) -> Vec<PathBuf> {
    let video = root.join(filename);
    let mut paths = vec![video.clone(), video.with_extension("info.json")];
    let Some(stem) = video.file_stem().and_then(|stem| stem.to_str()) else {
        return paths;
    };
    for extension in ["jpg", "jpeg", "png", "webp"] {
        paths.push(root.join(format!("{stem}.{extension}")));
    }
    let prefix = format!("{stem}.");
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with(&prefix)
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("vtt" | "srt" | "ass" | "ssa" | "sub")
                )
            {
                paths.push(path);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_accepts_only_plain_filenames() {
        assert!(is_safe_filename("episode.mp4"));
        assert!(!is_safe_filename("../episode.mp4"));
        assert!(!is_safe_filename("/tmp/episode.mp4"));
    }

    #[test]
    fn source_artifacts_are_limited_to_matching_stem() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("episode.en.vtt"), b"subtitle").unwrap();
        std::fs::write(root.path().join("other.en.vtt"), b"other").unwrap();
        let paths = source_artifacts(root.path(), "episode.mp4");
        assert!(paths.contains(&root.path().join("episode.mp4")));
        assert!(paths.contains(&root.path().join("episode.en.vtt")));
        assert!(!paths.contains(&root.path().join("other.en.vtt")));
    }
}

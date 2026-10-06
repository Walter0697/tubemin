use crate::{
    db,
    service_accounts::{RequireServiceAccount, ServiceScope},
    state::AppState,
    watcher,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::source_cleanup::{is_safe_filename, source_artifacts};

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
    RequireServiceAccount { principal }: RequireServiceAccount,
    State(state): State<AppState>,
    Json(request): Json<HandoffRequest>,
) -> impl IntoResponse {
    if let Err(response) = (RequireServiceAccount { principal })
        .require(ServiceScope::TransferComplete)
    {
        return response;
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

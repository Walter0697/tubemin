use crate::{db, peertube, state::AppState};
use axum::{extract::State, http::{header, HeaderMap, StatusCode}, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
pub struct CleanupRequest {
    /// When present, only these migrated videos are reconciled. Omit this
    /// field for an explicit full reconciliation.
    pub peertube_uuids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct CleanupResponse {
    pub checked: usize,
    pub deleted: usize,
    pub deleted_submission_ids: Vec<String>,
}

pub async fn cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CleanupRequest>,
) -> impl IntoResponse {
    let Some(expected) = state.config.peertube_cleanup_token.as_deref() else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "cleanup is disabled"}))).into_response();
    };
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied != Some(expected) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "invalid cleanup token"}))).into_response();
    }

    let (Some(url), Some(username), Some(password)) = (
        state.config.peertube_url.as_deref(),
        state.config.peertube_username.as_deref(),
        state.config.peertube_password.as_deref(),
    ) else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "PeerTube service account is not configured"}))).into_response();
    };

    let current = match peertube::list_account_videos(
        url,
        state.config.peertube_host.as_deref(),
        username,
        password,
        None,
    )
    .await
    {
        Ok(videos) => videos.into_iter().map(|video| video.uuid).collect::<HashSet<_>>(),
        Err(error) => {
            tracing::warn!(error = %error, "PeerTube cleanup inventory failed; no records removed");
            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": "PeerTube inventory failed"}))).into_response();
        }
    };

    let full_reconciliation = request.peertube_uuids.is_none();
    let candidates = request.peertube_uuids.unwrap_or_default();
    let stored = match db::all_peertube_uuids(&state.pool).await {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!(error = %error, "PeerTube cleanup database lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database lookup failed"}))).into_response();
        }
    };
    let scope = if full_reconciliation {
        stored
    } else {
        candidates.into_iter().collect::<HashSet<_>>()
    };
    let stale = scope
        .difference(&current)
        .cloned()
        .collect::<Vec<_>>();
    let deleted_submission_ids = match db::delete_submissions_by_peertube_uuids(&state.pool, &stale).await {
        Ok(ids) => ids,
        Err(error) => {
            tracing::error!(error = %error, "PeerTube cleanup database deletion failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database deletion failed"}))).into_response();
        }
    };

    Json(CleanupResponse {
        checked: scope.len(),
        deleted: deleted_submission_ids.len(),
        deleted_submission_ids,
    }).into_response()
}

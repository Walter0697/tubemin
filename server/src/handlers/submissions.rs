use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{db, oidc::RequireAuth, state::AppState};

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    pub status: Option<String>,
    pub q: Option<String>,
}
fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 24 }

#[derive(Serialize)]
pub struct SubmissionRow {
    pub id: String,
    pub url: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub peertube_thumb: Option<String>,
    pub peertube_uuid: Option<String>,
    pub status: String,
    pub submitted_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub submissions: Vec<SubmissionRow>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
    pub counts: std::collections::HashMap<String, i64>,
}

pub async fn list_submissions(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let per_page = q.per_page.clamp(1, 100);
    let page = q.page.max(1);
    let status_filter = q.status.as_deref().filter(|s| *s != "all");
    let search = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

    match db::list_submissions_paged(&state.pool, page, per_page, status_filter, search).await {
        Ok((rows, total, counts)) => {
            let submissions = rows.into_iter().map(|s| SubmissionRow {
                id: s.id,
                url: s.url,
                source_url: s.source_url,
                title: s.title,
                filename: s.filename,
                peertube_thumb: s.peertube_thumb,
                peertube_uuid: s.peertube_uuid,
                status: s.status,
                submitted_at: s.submitted_at,
                updated_at: s.updated_at,
            }).collect();
            Json(ListResponse { submissions, total, page, per_page, counts }).into_response()
        }
        Err(e) => {
            tracing::error!("DB error listing submissions: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DeleteRequest {
    pub ids: Vec<String>,
}

pub async fn delete_submissions(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<DeleteRequest>,
) -> impl IntoResponse {
    if body.ids.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "no ids provided"}))).into_response();
    }

    let uuids = match db::delete_submissions(&state.pool, &body.ids).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("DB error deleting submissions: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db error"}))).into_response();
        }
    };

    // Best-effort delete from PeerTube; failures are logged but don't block the response
    if let (Some(pt_url), Some(pt_user), Some(pt_pass)) = (
        &state.config.peertube_url,
        &state.config.peertube_username,
        &state.config.peertube_password,
    ) {
        for uuid in uuids.into_iter().flatten() {
            let url = pt_url.clone();
            let host = state.config.peertube_host.clone();
            let user = pt_user.clone();
            let pass = pt_pass.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::peertube::delete_video(&url, host.as_deref(), &user, &pass, &uuid).await {
                    tracing::warn!("PeerTube delete failed for {}: {}", uuid, e);
                }
            });
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"deleted": body.ids.len()}))).into_response()
}

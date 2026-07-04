use crate::{oidc::RequireAuth, state::AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct CheckSubmissionParams {
    pub url: String,
}

pub async fn check_submission(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    Query(params): Query<CheckSubmissionParams>,
) -> impl IntoResponse {
    match crate::db::get_submission_by_url(&state.pool, &params.url).await {
        Ok(Some(sub))
            if sub.submitter_sub.as_deref() == user.stable_subject()
                || (sub.submitter_sub.is_none()
                    && user.stable_subject().is_none()
                    && sub.submitter_display.as_deref() == Some(user.owner_display())) =>
        {
            (StatusCode::OK, Json(json!({"status": sub.status}))).into_response()
        }
        _ => (StatusCode::OK, Json(json!({"status": null}))).into_response(),
    }
}

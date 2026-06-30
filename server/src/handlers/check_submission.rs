use axum::{extract::{Query, State}, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct CheckSubmissionParams {
    pub url: String,
}

pub async fn check_submission(
    State(state): State<AppState>,
    Query(params): Query<CheckSubmissionParams>,
) -> impl IntoResponse {
    match crate::db::get_submission_by_url(&state.pool, &params.url).await {
        Ok(Some(sub)) => (StatusCode::OK, Json(json!({"status": sub.status}))).into_response(),
        _ => (StatusCode::OK, Json(json!({"status": null}))).into_response(),
    }
}

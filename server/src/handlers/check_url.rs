use axum::{extract::Query, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct CheckUrlParams {
    pub url: String,
}

pub async fn check_url(Query(params): Query<CheckUrlParams>) -> impl IntoResponse {
    if crate::url_validator::is_supported_url(&params.url) {
        (StatusCode::OK, Json(json!({"supported": true}))).into_response()
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"supported": false}))).into_response()
    }
}

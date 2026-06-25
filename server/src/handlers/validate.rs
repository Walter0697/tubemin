use axum::{extract::State, http::{HeaderMap, StatusCode}, response::IntoResponse};
use crate::{api_keys, state::AppState};

pub async fn validate(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let key = match headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(k) => k.to_string(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };
    match api_keys::verify_key(&state.pool, &key).await {
        Ok(Some(_)) => StatusCode::OK.into_response(),
        Ok(None) => StatusCode::UNAUTHORIZED.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

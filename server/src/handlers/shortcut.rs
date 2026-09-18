use crate::{api_keys, state::AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;

fn request_origin(headers: &HeaderMap) -> Option<String> {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

pub async fn setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let Some(origin) = request_origin(&headers) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "missing host"})),
        )
            .into_response();
    };

    let Some(owner) = (match api_keys::consume_shortcut_setup_token(&state.pool, &token).await {
        Ok(owner) => owner,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "database error"})),
            )
                .into_response();
        }
    }) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or expired setup token"})),
        )
            .into_response();
    };

    let api_key = match api_keys::generate(
        &state.pool,
        Some("iOS Shortcut"),
        api_keys::ApiKeyOwner {
            sub: owner.sub.as_deref(),
            display: &owner.display,
        },
    )
    .await
    {
        Ok(api_key) => api_key,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "could not create shortcut key"})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "submit_url": format!("{origin}/api/submit"),
            "api_key": api_key,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api_keys, config::Config, db, progress, state::AppState};
    use axum::{body::to_bytes, http::HeaderMap};
    use serde_json::Value;
    use std::{path::PathBuf, sync::Arc};

    fn test_state(pool: Arc<sqlx::SqlitePool>) -> AppState {
        AppState {
            pool,
            config: Arc::new(Config {
                api_port: 3000,
                metube_url: "http://metube".into(),
                downloads_dir: PathBuf::from("/tmp/downloads"),
                peertube_import_dir: PathBuf::from("/tmp/import"),
                database_url: "sqlite::memory:".into(),
                auth_mode: crate::config::AuthMode::Password,
                admin_password: None,
                cookie_secure: false,
                oidc_issuer_url: None,
                oidc_client_id: None,
                oidc_client_secret: None,
                oidc_redirect_url: None,
                oidc_login_label: "Sign in".into(),
                peertube_url: None,
                peertube_host: None,
                peertube_username: None,
                peertube_password: None,
                peertube_admin_email: None,
                peertube_admin_username: None,
                peertube_admin_password: None,
                peertube_video_privacy: 4,
                peertube_oidc_issuer_url: None,
                peertube_oidc_client_id: None,
                peertube_oidc_client_secret: None,
                peertube_cleanup_token: None,
            }),
            progress: progress::new_progress_map(),
        }
    }

    #[tokio::test]
    async fn setup_exchange_returns_submit_url_and_shortcut_key() {
        let pool = Arc::new(db::init("sqlite::memory:").await.unwrap());
        let state = test_state(pool.clone());
        let token = api_keys::create_shortcut_setup_token(
            &pool,
            api_keys::ApiKeyOwner {
                sub: Some("user-1"),
                display: "Walter",
            },
        )
        .await
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("host", "tubemin.example.test".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());

        let response = setup(State(state), headers, Path(token)).await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["submit_url"], "https://tubemin.example.test/api/submit");
        assert!(json["api_key"].as_str().unwrap().len() > 20);
    }

    #[tokio::test]
    async fn setup_exchange_rejects_unknown_token() {
        let pool = Arc::new(db::init("sqlite::memory:").await.unwrap());
        let state = test_state(pool);
        let mut headers = HeaderMap::new();
        headers.insert("host", "tubemin.example.test".parse().unwrap());
        let response = setup(State(state), headers, Path("not-a-token".into()))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}

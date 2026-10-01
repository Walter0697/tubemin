use crate::{db, oidc::RequireAuth, state::AppState};
use axum::{
    extract::{Request, State},
    response::{Html, IntoResponse},
    http::StatusCode,
    Json,
};
use minijinja::Environment;
use serde_json::json;
use std::sync::OnceLock;

static TRANSFERS_ENV: OnceLock<Environment<'static>> = OnceLock::new();

fn transfers_env() -> &'static Environment<'static> {
    TRANSFERS_ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        env.add_template("nav", include_str!("../../templates/partials/nav.html"))
            .unwrap();
        env.add_template("transfers", include_str!("../../templates/transfers.html"))
            .unwrap();
        env
    })
}

pub async fn transfers_page(
    RequireAuth(user): RequireAuth,
    State(_state): State<AppState>,
    _request: Request,
) -> Html<String> {
    let template = transfers_env().get_template("transfers").unwrap();
    Html(template.render(minijinja::context! {
        username => user.display_name(),
        active_page => "transfers",
        app_version => env!("CARGO_PKG_VERSION"),
        asset_version => crate::ASSET_VERSION,
    }).unwrap_or_else(|error| format!("Template error: {error}")))
}

pub async fn transfers_api(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match (db::list_transfers(&state.pool).await, db::count_archived_transfer_history(&state.pool).await) {
        (Ok(transfers), Ok((deleted_transfers, handed_off_submissions))) => Json(json!({
            "transfers": transfers,
            "archiveCounts": {
                "deletedTransfers": deleted_transfers,
                "handedOffSubmissions": handed_off_submissions,
            }
        })),
        (Err(error), _) | (_, Err(error)) => {
            tracing::error!(error = %error, "transfer history lookup failed");
            Json(json!({
                "transfers": [],
                "archiveCounts": { "deletedTransfers": 0, "handedOffSubmissions": 0 },
                "error": "transfer history unavailable"
            }))
        }
    }
}

pub async fn clear_archived_transfers(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match db::clear_archived_transfer_history(&state.pool).await {
        Ok(result) => Json(json!({
            "deletedTransfers": result.deleted_transfers,
            "handedOffSubmissions": result.handed_off_submissions,
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "archived transfer cleanup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "archived transfer cleanup failed" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_page_renders_navigation_and_title() {
        let html = transfers_env()
            .get_template("transfers")
            .unwrap()
            .render(minijinja::context! {
                username => "admin",
                active_page => "transfers",
                app_version => env!("CARGO_PKG_VERSION"),
                asset_version => crate::ASSET_VERSION,
            })
            .unwrap();

        assert!(html.contains("Transfer history"));
        assert!(html.contains(r#"href="/transfers""#));
    }
}

use axum::{
    extract::{Path, Query, State},
    response::{Html, Redirect},
};
use minijinja::Environment;
use serde::Deserialize;
use crate::{api_keys, oidc::RequireAuth, state::AppState};

#[derive(Deserialize)]
pub struct NewKeyQuery {
    pub new_key: Option<String>,
}

pub async fn settings(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Query(query): Query<NewKeyQuery>,
) -> Html<String> {
    let keys = api_keys::list(&state.pool).await.unwrap_or_default();

    let mut env = Environment::new();
    env.add_template("settings", include_str!("../../templates/settings.html")).unwrap();
    let tmpl = env.get_template("settings").unwrap();

    let ctx = minijinja::context! {
        new_key => query.new_key,
        api_keys => keys.iter().map(|k| minijinja::context! {
            id => k.id,
            label => k.label,
            created_at => k.created_at,
            last_used_at => k.last_used_at,
        }).collect::<Vec<_>>(),
    };

    Html(tmpl.render(ctx).unwrap_or_else(|e| format!("Template error: {}", e)))
}

pub async fn generate_key(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> Redirect {
    match api_keys::generate(&state.pool, Some("web-generated")).await {
        Ok(plaintext) => Redirect::to(&format!("/settings?new_key={}", plaintext)),
        Err(_) => Redirect::to("/settings"),
    }
}

pub async fn revoke_key(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Redirect {
    let _ = api_keys::revoke(&state.pool, &id).await;
    Redirect::to("/settings")
}

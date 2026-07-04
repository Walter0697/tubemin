use crate::{api_keys, oidc::RequireAuth, state::AppState};
use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, Redirect},
};
use minijinja::Environment;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct NewKeyQuery {
    pub new_key: Option<String>,
}

#[derive(Deserialize)]
pub struct CsrfForm {
    pub csrf_token: String,
}

const CSRF_SESSION_KEY: &str = "settings_csrf";

fn generate_csrf_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn settings(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    session: tower_sessions::Session,
    Query(query): Query<NewKeyQuery>,
) -> Html<String> {
    // Get or create CSRF token
    let csrf_token: String = match session.get(CSRF_SESSION_KEY).await.ok().flatten() {
        Some(t) => t,
        None => {
            let t = generate_csrf_token();
            session.insert(CSRF_SESSION_KEY, t.clone()).await.ok();
            t
        }
    };

    let keys = api_keys::list_by_owner(&state.pool, user.stable_subject(), user.display_name())
        .await
        .unwrap_or_default();

    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    env.add_template("nav", include_str!("../../templates/partials/nav.html"))
        .unwrap();
    env.add_template("settings", include_str!("../../templates/settings.html"))
        .unwrap();
    let tmpl = env.get_template("settings").unwrap();

    let ctx = minijinja::context! {
        username => user.display_name(),
        active_page => "settings",
        new_key => query.new_key,
        csrf_token => csrf_token,
        api_keys => keys.iter().map(|k| minijinja::context! {
            id => k.id,
            label => k.label,
            owner_display => k.owner_display,
            created_at => k.created_at,
            last_used_at => k.last_used_at,
        }).collect::<Vec<_>>(),
    };

    Html(
        tmpl.render(ctx)
            .unwrap_or_else(|e| format!("Template error: {}", e)),
    )
}

pub async fn generate_key(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    session: tower_sessions::Session,
    Form(form): Form<CsrfForm>,
) -> Redirect {
    // Validate CSRF
    let stored: Option<String> = session.get(CSRF_SESSION_KEY).await.ok().flatten();
    if stored.as_deref() != Some(&form.csrf_token) {
        return Redirect::to("/settings");
    }
    match api_keys::generate(
        &state.pool,
        Some("web-generated"),
        api_keys::ApiKeyOwner {
            sub: user.stable_subject(),
            display: user.display_name(),
        },
    )
    .await
    {
        Ok(plaintext) => Redirect::to(&format!("/settings?new_key={}", plaintext)),
        Err(_) => Redirect::to("/settings"),
    }
}

pub async fn revoke_key(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    session: tower_sessions::Session,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Redirect {
    // Validate CSRF
    let stored: Option<String> = session.get(CSRF_SESSION_KEY).await.ok().flatten();
    if stored.as_deref() != Some(&form.csrf_token) {
        return Redirect::to("/settings");
    }
    let _ = api_keys::revoke_by_owner(&state.pool, &id, user.stable_subject(), user.display_name())
        .await;
    Redirect::to("/settings")
}

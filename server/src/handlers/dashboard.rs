use axum::{extract::{State, Request}, response::Html};
use minijinja::Environment;
use crate::{db, oidc::RequireAuth, state::AppState};

pub async fn dashboard(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    req: Request,
) -> Html<String> {
    let submissions = db::list_submissions(&state.pool).await.unwrap_or_default();

    // Mirror the scheme of the incoming request: Caddy sets X-Forwarded-Proto: https
    // in production; local dev has no such header so we fall back to http.
    let scheme = req.headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");

    let peertube_base = state.config.peertube_host
        .as_ref()
        .map(|h| format!("{}://{}", scheme, h))
        .unwrap_or_default();

    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    env.add_template("dashboard", include_str!("../../templates/dashboard.html")).unwrap();
    let tmpl = env.get_template("dashboard").unwrap();

    let ctx = minijinja::context! {
        peertube_base => peertube_base,
        submissions => submissions.iter().map(|s| minijinja::context! {
            url => s.url,
            title => s.title,
            peertube_thumb => s.peertube_thumb,
            status => s.status,
            submitted_at => s.submitted_at,
        }).collect::<Vec<_>>(),
    };

    Html(tmpl.render(ctx).unwrap_or_else(|e| format!("Template error: {}", e)))
}

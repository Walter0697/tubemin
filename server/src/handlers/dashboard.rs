use axum::{extract::State, response::Html};
use minijinja::Environment;
use crate::{db, oidc::RequireAuth, state::AppState};

pub async fn dashboard(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> Html<String> {
    let submissions = db::list_submissions(&state.pool).await.unwrap_or_default();

    // Public-facing PeerTube base URL for constructing thumbnail URLs in the browser.
    // Uses http:// for localhost/IP (local dev), https:// for named hosts (production).
    let peertube_base = state.config.peertube_host
        .as_ref()
        .map(|h| {
            let is_local = h.starts_with("localhost") || h.starts_with("127.") || h.starts_with("192.168.");
            format!("{}://{}", if is_local { "http" } else { "https" }, h)
        })
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

use crate::{db, oidc::RequireAuth, state::AppState};
use axum::{
    extract::{Request, State},
    response::Html,
};
use minijinja::Environment;

static DASHBOARD_ENV: std::sync::OnceLock<Environment<'static>> = std::sync::OnceLock::new();

fn dashboard_env() -> &'static Environment<'static> {
    DASHBOARD_ENV.get_or_init(|| {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        env.add_template("nav", include_str!("../../templates/partials/nav.html"))
            .unwrap();
        env.add_template("dashboard", include_str!("../../templates/dashboard.html"))
            .unwrap();
        env
    })
}

pub async fn dashboard(
    RequireAuth(user): RequireAuth,
    State(state): State<AppState>,
    req: Request,
) -> Html<String> {
    let submissions =
        db::list_submissions_owned(&state.pool, user.stable_subject(), user.owner_display())
            .await
            .unwrap_or_default();

    // Mirror the scheme of the incoming request: Caddy sets X-Forwarded-Proto: https
    // in production; local dev has no such header so we fall back to http.
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");

    let peertube_base = state
        .config
        .peertube_host
        .as_ref()
        .map(|h| format!("{}://{}", scheme, h))
        .unwrap_or_default();

    let tmpl = dashboard_env().get_template("dashboard").unwrap();

    let ctx = minijinja::context! {
        peertube_base => peertube_base,
        username => user.display_name(),
        active_page => "dashboard",
        app_version => env!("CARGO_PKG_VERSION"),
        submissions => submissions.iter().map(|s| minijinja::context! {
            url => s.url,
            title => s.title,
            source_url => s.source_url,
            source => s.source,
            peertube_thumb => s.peertube_thumb,
            status => s.status,
            submitted_at => s.submitted_at,
        }).collect::<Vec<_>>(),
    };

    Html(
        tmpl.render(ctx)
            .unwrap_or_else(|e| format!("Template error: {}", e)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_renders_nav_with_username_and_active_tab() {
        let tmpl = dashboard_env().get_template("dashboard").unwrap();
        let html = tmpl
            .render(minijinja::context! {
                peertube_base => "",
                username => "admin",
                active_page => "dashboard",
                app_version => env!("CARGO_PKG_VERSION"),
            })
            .unwrap();
        assert!(html.contains(r#"<span class="nav-user">admin</span>"#));
        assert!(html.contains(r#"class="nav-link active""#));
        assert!(html.contains(env!("CARGO_PKG_VERSION")));
    }
}

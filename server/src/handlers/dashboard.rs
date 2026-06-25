use axum::{extract::State, response::Html};
use minijinja::Environment;
use crate::{db, oidc::RequireAuth, state::AppState};

pub async fn dashboard(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> Html<String> {
    let submissions = db::list_submissions(&state.pool).await.unwrap_or_default();

    let mut env = Environment::new();
    env.add_template("dashboard", include_str!("../../templates/dashboard.html")).unwrap();
    let tmpl = env.get_template("dashboard").unwrap();

    let ctx = minijinja::context! {
        submissions => submissions.iter().map(|s| minijinja::context! {
            url => s.url,
            status => s.status,
            submitted_at => s.submitted_at,
        }).collect::<Vec<_>>(),
    };

    Html(tmpl.render(ctx).unwrap_or_else(|e| format!("Template error: {}", e)))
}

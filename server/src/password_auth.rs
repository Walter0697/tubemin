use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    oidc::{OidcUser, SESSION_USER_KEY},
    state::AppState,
};

#[derive(Deserialize)]
pub struct LoginForm {
    pub password: String,
}

pub async fn login_form() -> Html<String> {
    Html(include_str!("../templates/login.html").replace("{{APP_VERSION}}", crate::ASSET_VERSION))
}

pub async fn login_submit(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let expected = state.config.admin_password.as_deref().unwrap_or("");
    if form.password == expected {
        session
            .insert(
                SESSION_USER_KEY,
                OidcUser {
                    sub: Some("password-admin".into()),
                    email: "admin".into(),
                    username: Some("admin".into()),
                },
            )
            .await
            .ok();
        Redirect::to("/dashboard").into_response()
    } else {
        Html(
            include_str!("../templates/login.html")
                .replace("{{APP_VERSION}}", crate::ASSET_VERSION)
                .replace("<!--ERROR-->", r#"<p class="error">Invalid password.</p>"#),
        )
        .into_response()
    }
}

pub async fn logout(session: Session) -> Redirect {
    session.delete().await.ok();
    Redirect::to("/auth/login")
}

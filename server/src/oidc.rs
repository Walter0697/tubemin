use axum::{
    async_trait,
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest::async_http_client,
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::state::AppState;

pub const SESSION_USER_KEY: &str = "oidc_user";
const SESSION_PKCE_KEY: &str = "pkce_verifier";
const SESSION_CSRF_KEY: &str = "csrf_token";
const SESSION_NONCE_KEY: &str = "nonce";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUser {
    #[serde(default)]
    pub sub: Option<String>,
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
}

impl OidcUser {
    /// Name shown in the UI: OIDC preferred_username when present, else email.
    pub fn display_name(&self) -> &str {
        self.username.as_deref().unwrap_or(&self.email)
    }

    pub fn stable_subject(&self) -> Option<&str> {
        self.sub.as_deref()
    }

    pub fn owner_display(&self) -> &str {
        self.display_name()
    }

    pub fn submitter_tag(&self) -> String {
        let raw = self
            .username
            .as_deref()
            .or_else(|| {
                self.email
                    .split_once('@')
                    .map(|(local, _)| local)
                    .filter(|local| !local.is_empty())
            })
            .or_else(|| self.sub.as_deref())
            .unwrap_or("unknown");

        let mut out = String::with_capacity(raw.len());
        let mut last_dash = false;
        for ch in raw.chars() {
            let mapped = if ch.is_ascii_alphanumeric() {
                last_dash = false;
                ch.to_ascii_lowercase()
            } else {
                if last_dash {
                    continue;
                }
                last_dash = true;
                '-'
            };
            out.push(mapped);
        }
        let out = out.trim_matches('-');
        if out.is_empty() {
            "unknown".into()
        } else {
            out.to_string()
        }
    }
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn build_oidc_client(config: &crate::config::Config) -> anyhow::Result<CoreClient> {
    let issuer = config
        .oidc_issuer_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OIDC_ISSUER_URL not configured"))?;
    let client_id = config
        .oidc_client_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OIDC_CLIENT_ID not configured"))?;
    let client_secret = config
        .oidc_client_secret
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OIDC_CLIENT_SECRET not configured"))?;
    let redirect_url = config
        .oidc_redirect_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OIDC_REDIRECT_URL not configured"))?;

    let provider_metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(issuer.to_string())?,
        async_http_client,
    )
    .await?;

    Ok(CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(client_id.to_string()),
        Some(ClientSecret::new(client_secret.to_string())),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_url.to_string())?))
}

pub async fn login_page(State(state): State<AppState>) -> impl IntoResponse {
    let label = &state.config.oidc_login_label;
    let html = include_str!("../templates/login_oidc.html").replace("{{OIDC_LOGIN_LABEL}}", label);
    Html(html)
}

pub async fn login(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let client = match build_oidc_client(&state.config).await {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".into()))
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    session
        .insert(SESSION_PKCE_KEY, pkce_verifier.secret().clone())
        .await
        .ok();
    session
        .insert(SESSION_CSRF_KEY, csrf_token.secret().clone())
        .await
        .ok();
    session
        .insert(SESSION_NONCE_KEY, nonce.secret().clone())
        .await
        .ok();

    Redirect::to(auth_url.as_str()).into_response()
}

pub async fn callback(
    State(state): State<AppState>,
    session: Session,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    let pkce_secret: String = match session.get(SESSION_PKCE_KEY).await.ok().flatten() {
        Some(v) => v,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let stored_csrf: String = match session.get(SESSION_CSRF_KEY).await.ok().flatten() {
        Some(v) => v,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };
    if stored_csrf != params.state {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let nonce_secret: String = match session.get(SESSION_NONCE_KEY).await.ok().flatten() {
        Some(v) => v,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let client = match build_oidc_client(&state.config).await {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let token_response = client
        .exchange_code(AuthorizationCode::new(params.code))
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_secret))
        .request_async(async_http_client)
        .await;

    match token_response {
        Ok(tokens) => {
            let id_token = match tokens.id_token() {
                Some(t) => t,
                None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };

            let nonce = Nonce::new(nonce_secret);
            let claims = match id_token.claims(&client.id_token_verifier(), &nonce) {
                Ok(c) => c,
                Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
            };

            let email = claims
                .email()
                .map(|e| e.as_str().to_string())
                .unwrap_or_else(|| "unknown".into());

            let username = claims.preferred_username().map(|u| u.as_str().to_string());
            let sub = Some(claims.subject().as_str().to_string());

            session
                .insert(
                    SESSION_USER_KEY,
                    OidcUser {
                        sub,
                        email,
                        username,
                    },
                )
                .await
                .ok();
            Redirect::to("/dashboard").into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}

pub async fn logout(session: Session) -> impl IntoResponse {
    session.delete().await.ok();
    Redirect::to("/auth/login")
}

pub struct RequireAuth(pub OidcUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| Redirect::to("/auth/login").into_response())?;

        match session.get::<OidcUser>(SESSION_USER_KEY).await {
            Ok(Some(user)) => Ok(RequireAuth(user)),
            _ => Err(Redirect::to("/auth/login").into_response()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_prefers_username() {
        let u = OidcUser {
            sub: None,
            email: "a@b.c".into(),
            username: Some("walter".into()),
        };
        assert_eq!(u.display_name(), "walter");
    }

    #[test]
    fn display_name_falls_back_to_email() {
        let u = OidcUser {
            sub: None,
            email: "a@b.c".into(),
            username: None,
        };
        assert_eq!(u.display_name(), "a@b.c");
    }

    #[test]
    fn old_session_json_deserializes_with_default_username() {
        let u: OidcUser = serde_json::from_str(r#"{"email":"a@b.c"}"#).unwrap();
        assert!(u.sub.is_none());
        assert!(u.username.is_none());
        assert_eq!(u.display_name(), "a@b.c");
    }

    #[test]
    fn submitter_tag_normalizes_username() {
        let u = OidcUser {
            sub: None,
            email: "a@b.c".into(),
            username: Some("Walter Smith".into()),
        };
        assert_eq!(u.submitter_tag(), "walter-smith");
    }
}

use axum::{
    async_trait,
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest::async_http_client,
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
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
    pub email: String,
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn build_oidc_client(config: &crate::config::Config) -> anyhow::Result<CoreClient> {
    let provider_metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(config.oidc_issuer_url.clone())?,
        async_http_client,
    )
    .await?;

    Ok(CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(config.oidc_client_id.clone()),
        Some(ClientSecret::new(config.oidc_client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(config.oidc_redirect_url.clone())?))
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

            session
                .insert(SESSION_USER_KEY, OidcUser { email })
                .await
                .ok();
            Redirect::to("/dashboard").into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
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

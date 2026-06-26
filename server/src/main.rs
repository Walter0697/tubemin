mod api_keys;
mod config;
mod db;
mod handlers;
mod metube;
mod oidc;
mod password_auth;
mod peertube;
mod poller;
mod state;
mod url_validator;
mod video_meta;
mod watcher;

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
use tower_http::cors::{CorsLayer, Any};
use tower_http::services::ServeDir;
use tower_sessions::{MemoryStore, SessionManagerLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "tubemin=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();
    let config = config::Config::from_env()?;
    let pool = Arc::new(db::init(&config.database_url).await?);
    let config = Arc::new(config);

    let app_state = state::AppState {
        pool: pool.clone(),
        config: config.clone(),
    };

    // Auto-provision PeerTube bot account if admin credentials are provided
    if let (Some(url), Some(host), Some(admin_user), Some(admin_pass), Some(bot_user), Some(bot_pass)) = (
        &config.peertube_url,
        config.peertube_host.as_deref(),
        &config.peertube_admin_username,
        &config.peertube_admin_password,
        &config.peertube_username,
        &config.peertube_password,
    ) {
        if admin_user != bot_user {
            let bot_email = config.peertube_admin_email.as_deref()
                .and_then(|e| e.split('@').nth(1))
                .map(|domain| format!("{}@{}", bot_user, domain))
                .unwrap_or_else(|| format!("{}@peertube.example", bot_user));
            if let Err(e) = peertube::ensure_account(url, Some(host), admin_user, admin_pass, bot_user, bot_pass, &bot_email).await {
                tracing::warn!("PeerTube bot account provisioning failed (will retry on next start): {}", e);
            }
        }
    }

    // Poll MeTube queue to transition pending → downloading
    poller::start(config.metube_url.clone(), pool.clone());

    // Start file watcher
    let pt_config = match (&config.peertube_url, &config.peertube_username, &config.peertube_password) {
        (Some(url), Some(user), Some(pass)) => Some(watcher::PeerTubeConfig {
            url: url.clone(),
            host: config.peertube_host.clone(),
            username: user.clone(),
            password: pass.clone(),
        }),
        _ => None,
    };
    watcher::start(
        config.downloads_dir.clone(),
        config.peertube_import_dir.clone(),
        pool.clone(),
        pt_config,
    );

    // Session layer
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store);

    // Auth routes depend on configured mode
    let auth_router: Router<state::AppState> = match config.auth_mode {
        config::AuthMode::Oidc => {
            tracing::info!("Auth mode: OIDC");
            Router::new()
                .route("/auth/login", get(oidc::login))
                .route("/auth/callback", get(oidc::callback))
        }
        config::AuthMode::Password => {
            tracing::info!("Auth mode: password");
            Router::new()
                .route("/auth/login", get(password_auth::login_form).post(password_auth::login_submit))
                .route("/auth/logout", get(password_auth::logout))
        }
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { axum::response::Redirect::to("/auth/login") }))
        .route("/api/submit", post(handlers::submit))
        .route("/api/validate", get(handlers::validate))
        .route("/api/check-url", get(handlers::check_url))
        .route("/api/check-submission", get(handlers::check_submission))
        .nest_service("/static", ServeDir::new("static"))
        .merge(auth_router)
        .route("/dashboard", get(handlers::dashboard))
        .route("/settings", get(handlers::settings))
        .route("/settings/keys/generate", post(handlers::generate_key))
        .route("/settings/keys/:id/revoke", post(handlers::revoke_key))
        .layer(session_layer)
        .layer(cors)
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!("Tubemin listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

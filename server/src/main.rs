mod api_keys;
mod config;
mod db;
mod handlers;
mod metube;
mod oidc;
mod password_auth;
mod state;
mod watcher;

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
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

    // Start file watcher
    watcher::start(
        config.downloads_dir.clone(),
        config.peertube_import_dir.clone(),
        pool.clone(),
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

    let app = Router::new()
        .route("/", get(|| async { axum::response::Redirect::to("/auth/login") }))
        .route("/api/submit", post(handlers::submit))
        .route("/api/validate", get(handlers::validate))
        .merge(auth_router)
        .route("/dashboard", get(handlers::dashboard))
        .route("/settings", get(handlers::settings))
        .route("/settings/keys/generate", post(handlers::generate_key))
        .route("/settings/keys/:id/revoke", post(handlers::revoke_key))
        .layer(session_layer)
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!("Tubemin listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

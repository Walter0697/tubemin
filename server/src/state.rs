use std::sync::Arc;
use sqlx::SqlitePool;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<Config>,
}

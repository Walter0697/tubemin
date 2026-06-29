use std::sync::Arc;
use sqlx::SqlitePool;
use crate::config::Config;
use crate::progress::ProgressMap;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<Config>,
    pub progress: ProgressMap,
}

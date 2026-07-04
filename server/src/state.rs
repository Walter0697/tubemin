use crate::config::Config;
use crate::progress::ProgressMap;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<Config>,
    pub progress: ProgressMap,
}

use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, warn};

pub fn start(metube_url: String, pool: Arc<SqlitePool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            match crate::metube::get_queue_urls(&metube_url).await {
                Ok(urls) => {
                    for url in urls {
                        if let Err(e) = crate::db::mark_downloading(&pool, &url).await {
                            error!(error = %e, url = %url, "db error marking submission as downloading");
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "could not poll metube queue (will retry)");
                }
            }
        }
    })
}

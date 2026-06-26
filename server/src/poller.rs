use std::collections::HashSet;
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, warn};

pub fn start(metube_url: String, pool: Arc<SqlitePool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            match crate::metube::get_queue_state(&metube_url).await {
                Ok(state) => {
                    // URLs currently in MeTube's queue or pending — not errors
                    let live: HashSet<String> = state.active.iter()
                        .chain(state.pending.iter())
                        .map(|i| i.url.clone())
                        .collect();

                    for item in &state.active {
                        if let Err(e) = crate::db::mark_downloading(&pool, &item.url).await {
                            error!(error = %e, url = %item.url, "db error marking as downloading");
                        }
                    }

                    // Update titles for anything MeTube knows about
                    for item in state.active.iter().chain(state.pending.iter()) {
                        if let Some(title) = &item.title {
                            if let Err(e) = crate::db::update_submission_title(&pool, &item.url, title).await {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
                        }
                    }

                    // Mark errored downloads — skip URLs that are actively live (re-submitted)
                    for item in &state.errored {
                        if live.contains(&item.url) {
                            continue;
                        }
                        if let Err(e) = crate::db::mark_active_as_error_by_url(&pool, &item.url).await {
                            error!(error = %e, url = %item.url, "db error marking as error");
                        }
                        if let Some(title) = &item.title {
                            if let Err(e) = crate::db::update_submission_title(&pool, &item.url, title).await {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
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

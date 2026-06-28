use std::collections::HashSet;
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, warn};
use crate::progress::ProgressMap;

pub fn start(metube_url: String, pool: Arc<SqlitePool>, progress: ProgressMap) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            match crate::metube::get_queue_state(&metube_url).await {
                Ok(state) => {
                    let live: HashSet<String> = state.active.iter()
                        .chain(state.pending.iter())
                        .map(|i| i.url.clone())
                        .collect();

                    let mut active_sub_ids: HashSet<String> = HashSet::new();
                    for item in &state.active {
                        if let Err(e) = crate::db::mark_downloading(&pool, &item.url).await {
                            error!(error = %e, url = %item.url, "db error marking as downloading");
                        }
                        match crate::db::get_submission_by_url(&pool, &item.url).await {
                            Ok(Some(sub)) => {
                                active_sub_ids.insert(sub.id.clone());
                                if let Some(pct) = item.percent {
                                    crate::progress::set(&progress, &sub.id, (pct / 100.0) as f32);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => error!(error = %e, url = %item.url, "db error fetching sub for progress"),
                        }
                    }

                    for item in state.active.iter().chain(state.pending.iter()) {
                        if let Some(title) = &item.title {
                            if let Err(e) = crate::db::update_submission_title(&pool, &item.url, title).await {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
                        }
                    }

                    for item in &state.errored {
                        if live.contains(&item.url) { continue; }
                        if let Err(e) = crate::db::mark_active_as_error_by_url(&pool, &item.url).await {
                            error!(error = %e, url = %item.url, "db error marking as error");
                        }
                        if let Ok(Some(sub)) = crate::db::get_submission_by_url(&pool, &item.url).await {
                            crate::progress::remove(&progress, &sub.id);
                        }
                        if let Some(title) = &item.title {
                            if let Err(e) = crate::db::update_submission_title(&pool, &item.url, title).await {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
                        }
                    }

                    // Also keep progress entries for in-flight direct downloads (not in MeTube queue)
                    match sqlx::query_as::<_, (String,)>(
                        "SELECT id FROM submissions WHERE status IN ('pending', 'downloading') AND is_direct = 1"
                    )
                    .fetch_all(pool.as_ref())
                    .await {
                        Ok(rows) => {
                            for (id,) in rows { active_sub_ids.insert(id); }
                        }
                        Err(e) => error!(error = %e, "poller: db error fetching direct downloads for progress retain"),
                    }

                    // Prune progress entries for downloads that have left the active set
                    // (i.e., completed successfully — errored items are already cleaned above)
                    if let Ok(mut map) = progress.lock() {
                        map.retain(|id, _| active_sub_ids.contains(id));
                    }
                }
                Err(e) => {
                    warn!(error = %e, "could not poll metube queue (will retry)");
                }
            }
        }
    })
}

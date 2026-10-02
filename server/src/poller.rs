use crate::progress::ProgressMap;
use chrono::Utc;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

const MAX_DOWNLOAD_RETRIES: i64 = 3;

fn is_permanent_error(error: Option<&str>) -> bool {
    error
        .map(|message| message.to_ascii_lowercase().contains("conversion failed"))
        .unwrap_or(false)
}

fn should_update_before_retry(retry_claimed: bool, auto_update_on_failure: bool) -> bool {
    retry_claimed && auto_update_on_failure
}

pub fn start(
    metube_url: String,
    pool: Arc<SqlitePool>,
    progress: ProgressMap,
    update_url: Option<String>,
    update_token: Option<String>,
    auto_update_on_failure: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        let startup_at = Utc::now().to_rfc3339();
        let mut startup_recovery_done = false;
        loop {
            ticker.tick().await;
            match crate::metube::get_queue_state(&metube_url).await {
                Ok(state) => {
                    if !startup_recovery_done {
                        recover_pending_metube_submissions(&pool, &metube_url, &state, &startup_at)
                            .await;
                        startup_recovery_done = true;
                    }
                    let live: HashSet<String> = state
                        .active
                        .iter()
                        .chain(state.pending.iter())
                        .map(|i| i.url.clone())
                        .collect();

                    let mut active_sub_ids: HashSet<String> = HashSet::new();
                    for item in state.active.iter().chain(state.pending.iter()) {
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
                            Err(e) => {
                                error!(error = %e, url = %item.url, "db error fetching sub for progress")
                            }
                        }
                        if let Some(title) = &item.title {
                            if let Err(e) =
                                crate::db::update_submission_title(&pool, &item.url, title).await
                            {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
                        }
                    }

                    for item in &state.errored {
                        if live.contains(&item.url) {
                            continue;
                        }
                        if let Err(e) =
                            crate::db::mark_active_as_error_by_url(&pool, &item.url).await
                        {
                            error!(error = %e, url = %item.url, "db error marking as error");
                        }
                        if let Ok(Some(sub)) =
                            crate::db::get_submission_by_url(&pool, &item.url).await
                        {
                            crate::progress::remove(&progress, &sub.id);
                        }
                        if let Some(title) = &item.title {
                            if let Err(e) =
                                crate::db::update_submission_title(&pool, &item.url, title).await
                            {
                                error!(error = %e, url = %item.url, "db error updating title");
                            }
                        }
                        if is_permanent_error(item.error.as_deref()) {
                            warn!(
                                url = %item.url,
                                error = ?item.error,
                                "not retrying permanent MeTube conversion failure"
                            );
                            continue;
                        }
                        let retry_claimed = match crate::db::claim_metube_retry(
                            &pool,
                            &item.url,
                            MAX_DOWNLOAD_RETRIES,
                        )
                        .await
                        {
                            Ok(claimed) => claimed,
                            Err(e) => {
                                error!(
                                    error = %e,
                                    url = %item.url,
                                    "db error claiming MeTube retry"
                                );
                                false
                            }
                        };
                        if !retry_claimed {
                            continue;
                        }

                        let mut updated_before_retry = false;
                        if should_update_before_retry(retry_claimed, auto_update_on_failure) {
                            if let (Some(update_url), Some(update_token)) =
                                (update_url.as_deref(), update_token.as_deref())
                            {
                                match crate::metube::request_update(update_url, update_token).await
                                {
                                    Ok(()) => {
                                        updated_before_retry = wait_for_metube(&metube_url).await;
                                        if !updated_before_retry {
                                            warn!(url = %item.url, "MeTube did not become ready after yt-dlp update");
                                        }
                                    }
                                    Err(e) => warn!(
                                        error = %e,
                                        url = %item.url,
                                        "MeTube update request failed; using normal retry"
                                    ),
                                }
                            }
                        }
                        if let Err(e) = crate::metube::submit(&metube_url, &item.url).await {
                            let _ = crate::db::mark_pending_as_error_by_url(&pool, &item.url).await;
                            error!(
                                error = %e,
                                url = %item.url,
                                "failed to submit MeTube retry"
                            );
                        } else if updated_before_retry {
                            warn!(url = %item.url, "retrying failed MeTube download after yt-dlp update");
                        } else {
                            warn!(url = %item.url, "retrying failed MeTube download");
                        }
                    }

                    // Record MeTube's reported filenames so the watcher can match
                    // landed files to submissions exactly instead of guessing.
                    for item in &state.finished {
                        if let Err(e) =
                            crate::db::set_filename_by_url(&pool, &item.url, &item.filename).await
                        {
                            error!(error = %e, url = %item.url, "db error recording filename");
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

async fn recover_pending_metube_submissions(
    pool: &SqlitePool,
    metube_url: &str,
    state: &crate::metube::QueueState,
    startup_at: &str,
) {
    let known_urls: HashSet<&str> = state
        .active
        .iter()
        .chain(state.pending.iter())
        .map(|item| item.url.as_str())
        .chain(state.errored.iter().map(|item| item.url.as_str()))
        .chain(state.finished.iter().map(|item| item.url.as_str()))
        .collect();

    let urls = match crate::db::pending_metube_urls_before(pool, startup_at).await {
        Ok(urls) => urls,
        Err(error) => {
            error!(error = %error, "startup MeTube recovery database lookup failed");
            return;
        }
    };

    for url in urls {
        if known_urls.contains(url.as_str()) {
            continue;
        }
        match crate::metube::submit(metube_url, &url).await {
            Ok(()) => info!(url = %url, "startup recovery resubmitted pending MeTube download"),
            Err(error) => {
                warn!(error = %error, url = %url, "startup recovery could not resubmit MeTube download")
            }
        }
    }
}

async fn wait_for_metube(metube_url: &str) -> bool {
    tokio::time::sleep(Duration::from_secs(2)).await;
    for _ in 0..30 {
        if crate::metube::get_queue_state(metube_url).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::is_permanent_error;

    #[test]
    fn update_is_attempted_only_for_claimed_retry() {
        assert!(super::should_update_before_retry(true, true));
        assert!(!super::should_update_before_retry(false, true));
        assert!(!super::should_update_before_retry(true, false));
    }

    #[test]
    fn conversion_failures_are_not_retryable() {
        assert!(is_permanent_error(Some("Conversion failed!")));
        assert!(is_permanent_error(Some("ffmpeg: CONVERSION FAILED")));
        assert!(!is_permanent_error(Some(
            "HTTP Error 429: Too Many Requests"
        )));
        assert!(!is_permanent_error(None));
    }
}

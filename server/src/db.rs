use chrono::Utc;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::str::FromStr;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Submission {
    pub id: String,
    pub url: String,
    pub source_url: Option<String>,
    pub source: Option<String>,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub peertube_thumb: Option<String>,
    pub peertube_uuid: Option<String>,
    pub api_key_id: Option<String>,
    pub submitter_sub: Option<String>,
    pub submitter_display: Option<String>,
    pub submitter_tag: Option<String>,
    pub status: String,
    pub is_direct: bool,
    pub download_method: Option<String>,
    pub download_retries: i64,
    pub downloading_at: Option<String>,
    pub imported_at: Option<String>,
    pub transcoding_at: Option<String>,
    pub completed_at: Option<String>,
    pub submitted_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffItem {
    pub peertube_uuid: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffItemState {
    Claimed,
    AlreadyClaimed,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffItemResult {
    pub filename: String,
    pub state: HandoffItemState,
}

pub async fn init(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn create_submission(
    pool: &SqlitePool,
    id: &str,
    url: &str,
    source_url: Option<&str>,
    source: Option<&str>,
    is_direct: bool,
    title: Option<&str>,
    api_key_id: Option<&str>,
    submitter_sub: Option<&str>,
    submitter_display: Option<&str>,
    submitter_tag: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO submissions (id, url, source_url, source, title, status, is_direct, api_key_id, submitter_sub, submitter_display, submitter_tag, submitted_at, updated_at) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(url)
    .bind(source_url)
    .bind(source)
    .bind(title)
    .bind(is_direct)
    .bind(api_key_id)
    .bind(submitter_sub)
    .bind(submitter_display)
    .bind(submitter_tag)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_peertube_thumb(
    pool: &SqlitePool,
    filename: &str,
    thumb_path: &str,
    peertube_uuid: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE submissions SET peertube_thumb = ?, peertube_uuid = ?, updated_at = ? WHERE filename = ?")
        .bind(thumb_path)
        .bind(peertube_uuid)
        .bind(&now)
        .bind(filename)
        .execute(pool)
        .await?;
    Ok(())
}

/// Atomically claim a submission for downstream migration. Matching by the
/// filename is required because a queued import may not have a UUID yet;
/// when a UUID is supplied, a different non-null UUID never matches.
pub async fn claim_handoff_item(
    pool: &SqlitePool,
    item: &HandoffItem,
) -> Result<HandoffItemResult, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, status FROM submissions
         WHERE ((? IS NOT NULL AND filename = ?
                 AND (? IS NULL OR peertube_uuid = ? OR peertube_uuid IS NULL))
             OR (? IS NULL AND peertube_uuid = ?))
         ORDER BY submitted_at DESC LIMIT 1",
    )
    .bind(&item.filename)
    .bind(&item.filename)
    .bind(&item.peertube_uuid)
    .bind(&item.peertube_uuid)
    .bind(&item.filename)
    .bind(&item.peertube_uuid)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((id, status)) = row else {
        tx.commit().await?;
        return Ok(HandoffItemResult {
            filename: item.filename.clone().unwrap_or_default(),
            state: HandoffItemState::NotFound,
        });
    };

    if status == "handed_off" {
        tx.commit().await?;
        return Ok(HandoffItemResult {
            filename: item.filename.clone().unwrap_or_default(),
            state: HandoffItemState::AlreadyClaimed,
        });
    }

    sqlx::query("UPDATE submissions SET status = 'handed_off', updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(HandoffItemResult {
        filename: item.filename.clone().unwrap_or_default(),
        state: HandoffItemState::Claimed,
    })
}

pub async fn is_handed_off_by_filename(
    pool: &SqlitePool,
    filename: &str,
) -> Result<bool, sqlx::Error> {
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM submissions WHERE filename = ? ORDER BY submitted_at DESC LIMIT 1",
    )
    .bind(filename)
    .fetch_optional(pool)
    .await?;
    Ok(status.as_deref() == Some("handed_off"))
}


pub async fn delete_submissions_owned(
    pool: &SqlitePool,
    ids: &[String],
    owner_sub: Option<&str>,
    owner_display: &str,
) -> Result<Vec<Option<String>>, sqlx::Error> {
    let mut uuids = Vec::new();
    for id in ids {
        let row: Option<(Option<String>,)> = if let Some(sub) = owner_sub {
            sqlx::query_as(
                "SELECT peertube_uuid FROM submissions WHERE id = ? AND submitter_sub = ?",
            )
            .bind(id)
            .bind(sub)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT peertube_uuid FROM submissions WHERE id = ? AND submitter_sub IS NULL AND submitter_display = ?",
            )
            .bind(id)
            .bind(owner_display)
            .fetch_optional(pool)
            .await?
        };
        let Some((uuid,)) = row else {
            return Ok(Vec::new());
        };
        uuids.push(uuid);
        if let Some(sub) = owner_sub {
            sqlx::query("DELETE FROM submissions WHERE id = ? AND submitter_sub = ?")
                .bind(id)
                .bind(sub)
                .execute(pool)
                .await?;
        } else {
            sqlx::query("DELETE FROM submissions WHERE id = ? AND submitter_sub IS NULL AND submitter_display = ?")
                .bind(id)
                .bind(owner_display)
                .execute(pool)
                .await?;
        }
    }
    Ok(uuids)
}

/// Delete submission rows by their PeerTube UUIDs. This is used by the
/// internal cleanup endpoint after an authorized PeerTube inventory check.
pub async fn delete_submissions_by_peertube_uuids(
    pool: &SqlitePool,
    uuids: &[String],
) -> Result<Vec<String>, sqlx::Error> {
    if uuids.is_empty() {
        return Ok(Vec::new());
    }

    let mut tx = pool.begin().await?;
    let mut select = sqlx::QueryBuilder::new(
        "SELECT id FROM submissions WHERE peertube_uuid IN (",
    );
    {
        let mut separated = select.separated(", ");
        for uuid in uuids {
            separated.push_bind(uuid);
        }
    }
    select.push(")");
    let rows: Vec<(String,)> = select
        .build_query_as()
        .fetch_all(&mut *tx)
        .await?;
    let ids = rows.into_iter().map(|(id,)| id).collect::<Vec<_>>();

    let mut delete = sqlx::QueryBuilder::new("DELETE FROM submissions WHERE peertube_uuid IN (");
    {
        let mut separated = delete.separated(", ");
        for uuid in uuids {
            separated.push_bind(uuid);
        }
    }
    delete.push(")");
    delete.build().execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ids)
}

pub async fn update_submission_title(
    pool: &SqlitePool,
    url: &str,
    title: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE submissions SET title = ?, updated_at = ? WHERE url = ?")
        .bind(title)
        .bind(&now)
        .bind(url)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_active_as_error_by_url(pool: &SqlitePool, url: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE url = ? AND status IN ('pending', 'downloading')"
    )
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_pending_as_error_by_url(pool: &SqlitePool, url: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE url = ? AND status = 'pending'"
    )
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically claim the next retry for a failed MeTube download.
/// Returns false once the retry budget has been exhausted.
pub async fn claim_metube_retry(
    pool: &SqlitePool,
    url: &str,
    max_retries: i64,
) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE submissions
         SET status = 'pending', filename = NULL, download_method = NULL,
             downloading_at = NULL, imported_at = NULL, updated_at = ?,
             download_retries = download_retries + 1
         WHERE url = ? AND is_direct = 0 AND status = 'error'
           AND download_retries < ?",
    )
    .bind(&now)
    .bind(url)
    .bind(max_retries)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn mark_downloading(pool: &SqlitePool, url: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'downloading', downloading_at = ?, updated_at = ? WHERE url = ? AND status IN ('pending', 'interrupted')"
    )
    .bind(&now)
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark a specific submission imported by its URL (for direct downloads where the URL is known).
pub async fn mark_imported_by_url(
    pool: &SqlitePool,
    url: &str,
    filename: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'imported', filename = ?, imported_at = ?, updated_at = ? WHERE url = ? AND status IN ('pending', 'downloading')"
    )
    .bind(filename)
    .bind(&now)
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record the on-disk filename MeTube reported for a URL (from its socket
/// events or /history), so the watcher can match landed files to submissions
/// exactly. First writer wins; a resubmit clears filename and re-arms this.
pub async fn set_filename_by_url(
    pool: &SqlitePool,
    url: &str,
    filename: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET filename = ?, updated_at = ? WHERE url = ? AND filename IS NULL",
    )
    .bind(filename)
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_imported(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    // Prefer the submission whose MeTube-reported filename matches this file
    // exactly. 'error' and 'interrupted' are included so rows marked during a
    // MeTube/tubemin restart recover when their file eventually lands.
    let claimed = sqlx::query(
        "UPDATE submissions SET status = 'imported', imported_at = ?, updated_at = ?
         WHERE filename = ? AND is_direct = 0
           AND status IN ('pending', 'downloading', 'interrupted', 'error')",
    )
    .bind(&now)
    .bind(&now)
    .bind(filename)
    .execute(pool)
    .await?;
    if claimed.rows_affected() > 0 {
        return Ok(());
    }
    // Fallback when no filename was recorded (e.g. tubemin was down for the
    // whole download): match the oldest in-progress MeTube submission.
    // is_direct=1 rows are excluded because those are handled by
    // mark_imported_by_url with URL matching.
    sqlx::query(
        "UPDATE submissions SET status = 'imported', filename = ?, imported_at = ?, updated_at = ?
         WHERE id = (SELECT id FROM submissions WHERE status IN ('pending', 'downloading') AND is_direct = 0 AND filename IS NULL ORDER BY submitted_at ASC LIMIT 1)"
    )
    .bind(filename)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// On startup: direct downloads in 'downloading' had their ffmpeg process killed — mark as error.
/// MeTube downloads in 'downloading' are marked as interrupted until the poller sees them again.
pub async fn reset_interrupted_downloads(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE status = 'downloading' AND is_direct = 1"
    )
    .bind(&now)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE submissions SET status = 'interrupted', updated_at = ? WHERE status = 'downloading' AND is_direct = 0"
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_transcoding(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'transcoding', transcoding_at = ?, updated_at = ? WHERE peertube_uuid = ? AND status IN ('imported', 'error')"
    )
    .bind(&now)
    .bind(&now)
    .bind(peertube_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_complete(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'complete', completed_at = ?, updated_at = ? WHERE peertube_uuid = ? AND status IN ('imported', 'transcoding', 'error')"
    )
    .bind(&now)
    .bind(&now)
    .bind(peertube_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error_by_uuid(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE peertube_uuid = ? AND status IN ('imported', 'transcoding', 'error')"
    )
    .bind(&now)
    .bind(peertube_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', filename = ?, updated_at = ?
         WHERE id = (SELECT id FROM submissions WHERE status IN ('pending', 'downloading') AND is_direct = 0 ORDER BY submitted_at ASC LIMIT 1)"
    )
    .bind(filename)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_title_by_filename(
    pool: &SqlitePool,
    filename: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT title FROM submissions WHERE filename = ? LIMIT 1")
            .bind(filename)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(t,)| t))
}

pub async fn get_submitter_by_filename(
    pool: &SqlitePool,
    filename: &str,
) -> Result<(Option<String>, Option<String>), sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT submitter_display, submitter_tag FROM submissions WHERE filename = ? LIMIT 1",
    )
    .bind(filename)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or((None, None)))
}

pub async fn get_source_by_filename(
    pool: &SqlitePool,
    filename: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT source FROM submissions WHERE filename = ? LIMIT 1")
            .bind(filename)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(source,)| source))
}

pub async fn get_submission_by_url(
    pool: &SqlitePool,
    url: &str,
) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(
        "SELECT * FROM submissions WHERE url = ? ORDER BY submitted_at DESC LIMIT 1",
    )
    .bind(url)
    .fetch_optional(pool)
    .await
}

/// Record which download path (yt-dlp / ffmpeg-retry) is handling a direct download.
pub async fn set_download_method(
    pool: &SqlitePool,
    url: &str,
    method: &str,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE submissions SET download_method = ?, updated_at = ? WHERE url = ?")
        .bind(method)
        .bind(&now)
        .bind(url)
        .execute(pool)
        .await?;
    Ok(())
}

/// Reset an error row back to pending (for retry). Returns true if a row was updated.
pub async fn reset_submission_to_pending(
    pool: &SqlitePool,
    url: &str,
    api_key_id: Option<&str>,
    source: Option<&str>,
    submitter_sub: Option<&str>,
    submitter_display: Option<&str>,
    submitter_tag: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE submissions SET status = 'pending', filename = NULL, download_method = NULL, download_retries = 0, downloading_at = NULL, imported_at = NULL, transcoding_at = NULL, completed_at = NULL, api_key_id = ?, source = ?, submitter_sub = ?, submitter_display = ?, submitter_tag = ?, updated_at = ? WHERE url = ? AND status IN ('error', 'interrupted')"
    )
    .bind(api_key_id)
    .bind(source)
    .bind(submitter_sub)
    .bind(submitter_display)
    .bind(submitter_tag)
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_submissions_owned(
    pool: &SqlitePool,
    owner_sub: Option<&str>,
    owner_display: &str,
) -> Result<Vec<Submission>, sqlx::Error> {
    if let Some(sub) = owner_sub {
        Ok(sqlx::query_as::<_, Submission>(
            "SELECT * FROM submissions WHERE submitter_sub = ? ORDER BY submitted_at DESC",
        )
        .bind(sub)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, Submission>(
            "SELECT * FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? ORDER BY submitted_at DESC",
        )
        .bind(owner_display)
        .fetch_all(pool)
        .await?)
    }
}

pub async fn list_submissions(pool: &SqlitePool) -> Result<Vec<Submission>, sqlx::Error> {
    Ok(
        sqlx::query_as::<_, Submission>("SELECT * FROM submissions ORDER BY submitted_at DESC")
            .fetch_all(pool)
            .await?,
    )
}

pub async fn all_peertube_uuids(pool: &SqlitePool) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT peertube_uuid FROM submissions WHERE peertube_uuid IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(uuid,)| uuid).collect())
}

#[derive(sqlx::FromRow)]
struct StatusCount {
    status: String,
    count: i64,
}

pub async fn list_submissions_paged(
    pool: &SqlitePool,
    owner_sub: Option<&str>,
    owner_display: &str,
    page: u32,
    per_page: u32,
    status: Option<&str>,
    search: Option<&str>,
) -> Result<(Vec<Submission>, i64, std::collections::HashMap<String, i64>), sqlx::Error> {
    let offset = (page.saturating_sub(1)) as i64 * per_page as i64;
    let like = search.map(|q| format!("%{}%", q));

    let (rows, total) = match (owner_sub, status, like.as_deref()) {
        (Some(owner), Some(s), Some(q)) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub = ? AND status = ? AND (title LIKE ? OR url LIKE ?)",
            ).bind(owner).bind(s).bind(q).bind(q).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub = ? AND status = ? AND (title LIKE ? OR url LIKE ?) ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner).bind(s).bind(q).bind(q).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (Some(owner), Some(s), None) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub = ? AND status = ?",
            )
            .bind(owner)
            .bind(s)
            .fetch_one(pool)
            .await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub = ? AND status = ? ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner).bind(s).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (Some(owner), None, Some(q)) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub = ? AND (title LIKE ? OR url LIKE ?)",
            ).bind(owner).bind(q).bind(q).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub = ? AND (title LIKE ? OR url LIKE ?) ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner).bind(q).bind(q).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (Some(owner), None, None) => {
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM submissions WHERE submitter_sub = ?")
                    .bind(owner)
                    .fetch_one(pool)
                    .await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub = ? ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (None, Some(s), Some(q)) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND status = ? AND (title LIKE ? OR url LIKE ?)",
            ).bind(owner_display).bind(s).bind(q).bind(q).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND status = ? AND (title LIKE ? OR url LIKE ?) ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner_display).bind(s).bind(q).bind(q).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (None, Some(s), None) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND status = ?",
            ).bind(owner_display).bind(s).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND status = ? ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner_display).bind(s).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (None, None, Some(q)) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND (title LIKE ? OR url LIKE ?)",
            ).bind(owner_display).bind(q).bind(q).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? AND (title LIKE ? OR url LIKE ?) ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner_display).bind(q).bind(q).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
        (None, None, None) => {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ?",
            ).bind(owner_display).fetch_one(pool).await?;
            let rows = sqlx::query_as::<_, Submission>(
                "SELECT * FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? ORDER BY submitted_at DESC LIMIT ? OFFSET ?",
            ).bind(owner_display).bind(per_page as i64).bind(offset).fetch_all(pool).await?;
            (rows, total)
        }
    };

    let count_rows = if let Some(sub) = owner_sub {
        sqlx::query_as::<_, StatusCount>(
            "SELECT status, COUNT(*) as count FROM submissions WHERE submitter_sub = ? GROUP BY status",
        )
        .bind(sub)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, StatusCount>(
            "SELECT status, COUNT(*) as count FROM submissions WHERE submitter_sub IS NULL AND submitter_display = ? GROUP BY status",
        )
        .bind(owner_display)
        .fetch_all(pool)
        .await?
    };
    let mut counts: std::collections::HashMap<String, i64> = count_rows
        .into_iter()
        .map(|r| (r.status, r.count))
        .collect();
    let all: i64 = counts.values().sum();
    counts.insert("all".into(), all);

    Ok((rows, total, counts))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        init("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn create_and_list_submission() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "test-id",
            "https://example.com/video",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "pending");
    }

    #[tokio::test]
    async fn cleanup_deletes_rows_by_peertube_uuid_only() {
        let pool = test_pool().await;
        create_submission(&pool, "keep", "https://example.com/keep", None, None, false, None, None, None, None, None).await.unwrap();
        create_submission(&pool, "remove", "https://example.com/remove", None, None, false, None, None, None, None, None).await.unwrap();
        sqlx::query("UPDATE submissions SET peertube_uuid = ? WHERE id = ?")
            .bind("uuid-keep")
            .bind("keep")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE submissions SET peertube_uuid = ? WHERE id = ?")
            .bind("uuid-remove")
            .bind("remove")
            .execute(&pool)
            .await
            .unwrap();

        let deleted = delete_submissions_by_peertube_uuids(&pool, &["uuid-remove".into()]).await.unwrap();

        assert_eq!(deleted, vec!["remove"]);
        assert!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM submissions WHERE id = 'keep'")
            .fetch_one(&pool)
            .await
            .unwrap() == 1);
        assert!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM submissions WHERE id = 'remove'")
            .fetch_one(&pool)
            .await
            .unwrap() == 0);
    }

    #[tokio::test]
    async fn handoff_claims_matching_submission_and_is_idempotent() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "handoff-id",
            "https://example.com/handoff",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        mark_imported(&pool, "handoff.mp4").await.unwrap();
        set_peertube_thumb(&pool, "handoff.mp4", "/thumb.jpg", "uuid-handoff")
            .await
            .unwrap();

        let item = HandoffItem {
            peertube_uuid: Some("uuid-handoff".into()),
            filename: Some("handoff.mp4".into()),
        };
        assert_eq!(
            claim_handoff_item(&pool, &item).await.unwrap().state,
            HandoffItemState::Claimed
        );
        assert_eq!(
            claim_handoff_item(&pool, &item).await.unwrap().state,
            HandoffItemState::AlreadyClaimed
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT status FROM submissions WHERE id = 'handoff-id'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "handed_off"
        );
    }

    #[tokio::test]
    async fn handoff_can_claim_import_without_peertube_uuid() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "queued-handoff",
            "https://example.com/queued",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        mark_imported(&pool, "queued.mp4").await.unwrap();

        let result = claim_handoff_item(
            &pool,
            &HandoffItem {
                peertube_uuid: None,
                filename: Some("queued.mp4".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.state, HandoffItemState::Claimed);
    }

    #[tokio::test]
    async fn handoff_does_not_match_different_peertube_uuid() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "wrong-uuid",
            "https://example.com/wrong",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        mark_imported(&pool, "same-name.mp4").await.unwrap();
        set_peertube_thumb(&pool, "same-name.mp4", "/thumb.jpg", "stored-uuid")
            .await
            .unwrap();

        let result = claim_handoff_item(
            &pool,
            &HandoffItem {
                peertube_uuid: Some("other-uuid".into()),
                filename: Some("same-name.mp4".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.state, HandoffItemState::NotFound);
    }

    #[tokio::test]
    async fn handoff_state_is_visible_to_watcher_lookup() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "watcher-handoff",
            "https://example.com/watcher",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        mark_imported(&pool, "watcher.mp4").await.unwrap();
        claim_handoff_item(
            &pool,
            &HandoffItem {
                peertube_uuid: None,
                filename: Some("watcher.mp4".into()),
            },
        )
        .await
        .unwrap();

        assert!(is_handed_off_by_filename(&pool, "watcher.mp4")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn mark_imported_updates_status() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "test-id-2",
            "https://example.com/video2",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        mark_imported(&pool, "video.mp4").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "imported");
        assert_eq!(rows[0].filename.as_deref(), Some("video.mp4"));
    }

    async fn create_basic(pool: &SqlitePool, id: &str, url: &str) {
        create_submission(pool, id, url, None, None, false, None, None, None, None, None)
            .await
            .unwrap();
        // submitted_at has second precision in RFC3339; force distinct ordering
        sqlx::query("UPDATE submissions SET submitted_at = ? WHERE id = ?")
            .bind(format!("2026-01-01T00:00:{:02}Z", {
                let n: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM submissions")
                        .fetch_one(pool)
                        .await
                        .unwrap();
                n
            }))
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn status_of(pool: &SqlitePool, id: &str) -> (String, Option<String>) {
        sqlx::query_as("SELECT status, filename FROM submissions WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn metube_retry_budget_is_three() {
        let pool = test_pool().await;
        create_basic(&pool, "retry", "https://example.com/retry").await;
        sqlx::query("UPDATE submissions SET status = 'error' WHERE id = 'retry'")
            .execute(&pool)
            .await
            .unwrap();

        for expected in 1..=3 {
            assert!(claim_metube_retry(&pool, "https://example.com/retry", 3)
                .await
                .unwrap());
            let retries: i64 = sqlx::query_scalar(
                "SELECT download_retries FROM submissions WHERE id = 'retry'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(retries, expected);
            sqlx::query("UPDATE submissions SET status = 'error' WHERE id = 'retry'")
                .execute(&pool)
                .await
                .unwrap();
        }

        assert!(!claim_metube_retry(&pool, "https://example.com/retry", 3)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn set_filename_by_url_first_writer_wins() {
        let pool = test_pool().await;
        create_basic(&pool, "a", "https://example.com/a").await;
        set_filename_by_url(&pool, "https://example.com/a", "a.webm")
            .await
            .unwrap();
        set_filename_by_url(&pool, "https://example.com/a", "other.webm")
            .await
            .unwrap();
        let (_, filename) = status_of(&pool, "a").await;
        assert_eq!(filename.as_deref(), Some("a.webm"));
    }

    // Regression: downloads finishing out of submission order must not swap rows.
    #[tokio::test]
    async fn mark_imported_prefers_exact_filename_match() {
        let pool = test_pool().await;
        create_basic(&pool, "older", "https://example.com/older").await;
        create_basic(&pool, "newer", "https://example.com/newer").await;
        mark_downloading(&pool, "https://example.com/older").await.unwrap();
        mark_downloading(&pool, "https://example.com/newer").await.unwrap();
        set_filename_by_url(&pool, "https://example.com/newer", "newer.webm")
            .await
            .unwrap();

        // The newer submission's file lands first
        mark_imported(&pool, "newer.webm").await.unwrap();

        let (newer_status, newer_file) = status_of(&pool, "newer").await;
        assert_eq!(newer_status, "imported");
        assert_eq!(newer_file.as_deref(), Some("newer.webm"));
        let (older_status, older_file) = status_of(&pool, "older").await;
        assert_eq!(older_status, "downloading", "older row must not claim the file");
        assert_eq!(older_file, None);
    }

    #[tokio::test]
    async fn mark_imported_falls_back_to_oldest_without_recorded_filename() {
        let pool = test_pool().await;
        create_basic(&pool, "older", "https://example.com/older").await;
        create_basic(&pool, "newer", "https://example.com/newer").await;
        mark_downloading(&pool, "https://example.com/older").await.unwrap();
        mark_downloading(&pool, "https://example.com/newer").await.unwrap();

        mark_imported(&pool, "mystery.webm").await.unwrap();

        let (older_status, older_file) = status_of(&pool, "older").await;
        assert_eq!(older_status, "imported");
        assert_eq!(older_file.as_deref(), Some("mystery.webm"));
    }

    // A row stuck in 'error' (e.g. marked during a MeTube restart) must recover
    // when its file eventually lands.
    #[tokio::test]
    async fn mark_imported_revives_errored_row_with_matching_filename() {
        let pool = test_pool().await;
        create_basic(&pool, "e1", "https://example.com/e1").await;
        sqlx::query("UPDATE submissions SET status='error' WHERE id='e1'")
            .execute(&pool)
            .await
            .unwrap();
        set_filename_by_url(&pool, "https://example.com/e1", "e1.webm")
            .await
            .unwrap();

        mark_imported(&pool, "e1.webm").await.unwrap();

        let (status, _) = status_of(&pool, "e1").await;
        assert_eq!(status, "imported");
    }

    #[tokio::test]
    async fn mark_transcoding_transitions_imported() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "t1",
            "https://example.com/v",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        // Simulate imported with a peertube_uuid
        sqlx::query(
            "UPDATE submissions SET status='imported', peertube_uuid='uuid-abc' WHERE id='t1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        mark_transcoding(&pool, "uuid-abc").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "transcoding");
    }

    #[tokio::test]
    async fn mark_complete_transitions_transcoding() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "t2",
            "https://example.com/v2",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE submissions SET status='transcoding', peertube_uuid='uuid-xyz' WHERE id='t2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        mark_complete(&pool, "uuid-xyz").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "complete");
    }

    #[tokio::test]
    async fn mark_imported_skips_direct_downloads() {
        let pool = test_pool().await;
        // Direct download submitted first (older), MeTube submission second
        create_submission(
            &pool,
            "direct-id",
            "https://cdn.example.com/video.m3u8",
            None,
            None,
            true,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        create_submission(
            &pool,
            "metube-id",
            "https://www.youtube.com/watch?v=abc",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // MeTube download completes — mark_imported must NOT grab the direct row
        mark_imported(&pool, "youtube_video.mp4").await.unwrap();

        let rows = list_submissions(&pool).await.unwrap();
        let direct = rows.iter().find(|r| r.id == "direct-id").unwrap();
        let metube = rows.iter().find(|r| r.id == "metube-id").unwrap();

        assert_eq!(
            direct.status, "pending",
            "direct download row must not be touched by mark_imported"
        );
        assert_eq!(
            metube.status, "imported",
            "metube row should be marked imported"
        );
        assert_eq!(metube.filename.as_deref(), Some("youtube_video.mp4"));
    }

    #[tokio::test]
    async fn reset_interrupted_downloads_marks_metube_rows_interrupted() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "metube-id",
            "https://www.youtube.com/watch?v=abc",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE submissions SET status='downloading' WHERE id='metube-id'")
            .execute(&pool)
            .await
            .unwrap();

        reset_interrupted_downloads(&pool).await.unwrap();

        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "interrupted");
    }

    #[tokio::test]
    async fn mark_downloading_revives_interrupted_rows() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "metube-id",
            "https://www.youtube.com/watch?v=abc",
            None,
            None,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE submissions SET status='interrupted' WHERE id='metube-id'")
            .execute(&pool)
            .await
            .unwrap();

        mark_downloading(&pool, "https://www.youtube.com/watch?v=abc")
            .await
            .unwrap();

        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "downloading");
    }

    #[tokio::test]
    async fn get_submitter_by_filename_returns_snapshot_fields() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "test-id-3",
            "https://example.com/video3",
            None,
            None,
            false,
            None,
            Some("key-1"),
            Some("user-1"),
            Some("Walter"),
            Some("walter"),
        )
        .await
        .unwrap();
        mark_imported(&pool, "video3.mp4").await.unwrap();
        let (display, tag) = get_submitter_by_filename(&pool, "video3.mp4")
            .await
            .unwrap();
        assert_eq!(display.as_deref(), Some("Walter"));
        assert_eq!(tag.as_deref(), Some("walter"));
    }

    #[tokio::test]
    async fn list_submissions_owned_only_returns_matching_owner() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "user-1-sub",
            "https://example.com/u1",
            None,
            None,
            false,
            None,
            Some("key-1"),
            Some("user-1"),
            Some("Walter"),
            Some("walter"),
        )
        .await
        .unwrap();
        create_submission(
            &pool,
            "user-2-sub",
            "https://example.com/u2",
            None,
            None,
            false,
            None,
            Some("key-2"),
            Some("user-2"),
            Some("Alice"),
            Some("alice"),
        )
        .await
        .unwrap();

        let rows = list_submissions_owned(&pool, Some("user-1"), "Walter")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "user-1-sub");
    }

    #[tokio::test]
    async fn delete_submissions_owned_rejects_foreign_rows() {
        let pool = test_pool().await;
        create_submission(
            &pool,
            "user-1-sub",
            "https://example.com/u1",
            None,
            None,
            false,
            None,
            Some("key-1"),
            Some("user-1"),
            Some("Walter"),
            Some("walter"),
        )
        .await
        .unwrap();
        create_submission(
            &pool,
            "user-2-sub",
            "https://example.com/u2",
            None,
            None,
            false,
            None,
            Some("key-2"),
            Some("user-2"),
            Some("Alice"),
            Some("alice"),
        )
        .await
        .unwrap();

        let deleted = delete_submissions_owned(
            &pool,
            &[String::from("user-2-sub")],
            Some("user-1"),
            "Walter",
        )
        .await
        .unwrap();
        assert!(deleted.is_empty());
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
    }
}

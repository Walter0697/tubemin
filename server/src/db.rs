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
    pub submitted_at: String,
    pub updated_at: String,
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

pub async fn mark_downloading(pool: &SqlitePool, url: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'downloading', updated_at = ? WHERE url = ? AND status = 'pending'"
    )
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
        "UPDATE submissions SET status = 'imported', filename = ?, updated_at = ? WHERE url = ? AND status IN ('pending', 'downloading')"
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
    // yt-dlp filenames can't be mapped back to the submitted URL, so we match
    // the oldest in-progress MeTube submission. is_direct=1 rows are excluded
    // because those are handled by mark_imported_by_url with URL matching.
    sqlx::query(
        "UPDATE submissions SET status = 'imported', filename = ?, updated_at = ?
         WHERE id = (SELECT id FROM submissions WHERE status IN ('pending', 'downloading') AND is_direct = 0 ORDER BY submitted_at ASC LIMIT 1)"
    )
    .bind(filename)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// On startup: direct downloads in 'downloading' had their ffmpeg process killed — mark as error.
/// MeTube downloads in 'downloading' are reset to 'pending' so the poller can re-detect them.
pub async fn reset_interrupted_downloads(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE status = 'downloading' AND is_direct = 1"
    )
    .bind(&now)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE submissions SET status = 'pending', updated_at = ? WHERE status = 'downloading' AND is_direct = 0"
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_transcoding(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'transcoding', updated_at = ? WHERE peertube_uuid = ? AND status = 'imported'"
    )
    .bind(&now)
    .bind(peertube_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_complete(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'complete', updated_at = ? WHERE peertube_uuid = ? AND status IN ('imported', 'transcoding')"
    )
    .bind(&now)
    .bind(peertube_uuid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error_by_uuid(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE peertube_uuid = ? AND status IN ('imported', 'transcoding')"
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
        "UPDATE submissions SET status = 'pending', filename = NULL, api_key_id = ?, source = ?, submitter_sub = ?, submitter_display = ?, submitter_tag = ?, updated_at = ? WHERE url = ? AND status = 'error'"
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

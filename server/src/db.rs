use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;
use chrono::Utc;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Submission {
    pub id: String,
    pub url: String,
    pub filename: Option<String>,
    pub status: String,
    pub submitted_at: String,
    pub updated_at: String,
}

pub async fn init(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn create_submission(pool: &SqlitePool, id: &str, url: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO submissions (id, url, status, submitted_at, updated_at) VALUES (?, ?, 'pending', ?, ?)"
    )
    .bind(id)
    .bind(url)
    .bind(&now)
    .bind(&now)
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

pub async fn mark_imported(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    // yt-dlp filenames can't be mapped back to the submitted URL, so we match
    // the oldest in-progress submission — correct for a single-user sequential queue.
    sqlx::query(
        "UPDATE submissions SET status = 'imported', filename = ?, updated_at = ?
         WHERE id = (SELECT id FROM submissions WHERE status IN ('pending', 'downloading') ORDER BY submitted_at ASC LIMIT 1)"
    )
    .bind(filename)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', filename = ?, updated_at = ?
         WHERE id = (SELECT id FROM submissions WHERE status IN ('pending', 'downloading') ORDER BY submitted_at ASC LIMIT 1)"
    )
    .bind(filename)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_submission_by_url(pool: &SqlitePool, url: &str) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(
        "SELECT * FROM submissions WHERE url = ? ORDER BY submitted_at DESC LIMIT 1"
    )
    .bind(url)
    .fetch_optional(pool)
    .await
}

/// Reset an error row back to pending (for retry). Returns true if a row was updated.
pub async fn reset_submission_to_pending(pool: &SqlitePool, url: &str) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE submissions SET status = 'pending', filename = NULL, updated_at = ? WHERE url = ? AND status = 'error'"
    )
    .bind(&now)
    .bind(url)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_submissions(pool: &SqlitePool) -> Result<Vec<Submission>, sqlx::Error> {
    Ok(sqlx::query_as::<_, Submission>(
        "SELECT * FROM submissions ORDER BY submitted_at DESC"
    )
    .fetch_all(pool)
    .await?)
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
        create_submission(&pool, "test-id", "https://example.com/video").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "pending");
    }

    #[tokio::test]
    async fn mark_imported_updates_status() {
        let pool = test_pool().await;
        create_submission(&pool, "test-id-2", "https://example.com/video2").await.unwrap();
        mark_imported(&pool, "video.mp4").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "imported");
        assert_eq!(rows[0].filename.as_deref(), Some("video.mp4"));
    }
}

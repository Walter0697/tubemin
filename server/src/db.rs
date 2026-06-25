use sqlx::SqlitePool;
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
    let pool = SqlitePool::connect(database_url).await?;
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

pub async fn mark_imported(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'imported', updated_at = ? WHERE filename = ?"
    )
    .bind(&now)
    .bind(filename)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE submissions SET status = 'error', updated_at = ? WHERE filename = ?"
    )
    .bind(&now)
    .bind(filename)
    .execute(pool)
    .await?;
    Ok(())
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
        // set filename first
        sqlx::query("UPDATE submissions SET filename = 'video.mp4' WHERE id = 'test-id-2'")
            .execute(&pool)
            .await
            .unwrap();
        mark_imported(&pool, "video.mp4").await.unwrap();
        let rows = list_submissions(&pool).await.unwrap();
        assert_eq!(rows[0].status, "imported");
    }
}

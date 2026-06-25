use sqlx::SqlitePool;
use bcrypt::{hash, verify, DEFAULT_COST};
use uuid::Uuid;
use chrono::Utc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiKeyError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("bcrypt error: {0}")]
    Bcrypt(#[from] bcrypt::BcryptError),
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKey {
    pub id: String,
    pub label: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

pub async fn generate(pool: &SqlitePool, label: Option<&str>) -> Result<String, ApiKeyError> {
    let plaintext = Uuid::new_v4().to_string() + "-" + &Uuid::new_v4().to_string();
    let key_hash = hash(&plaintext, DEFAULT_COST)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO api_keys (id, key_hash, label, created_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&key_hash)
    .bind(label)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(plaintext)
}

pub async fn verify_key(pool: &SqlitePool, plaintext: &str) -> Result<Option<String>, ApiKeyError> {
    let keys = sqlx::query_as::<_, (String, String)>(
        "SELECT id, key_hash FROM api_keys"
    )
    .fetch_all(pool)
    .await?;

    let plaintext = plaintext.to_string();
    let result = tokio::task::spawn_blocking(move || {
        for (id, key_hash) in keys {
            if verify(&plaintext, &key_hash).unwrap_or(false) {
                return Some(id);
            }
        }
        None
    })
    .await
    .unwrap_or(None);

    Ok(result)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ApiKey>, ApiKeyError> {
    Ok(sqlx::query_as::<_, ApiKey>(
        "SELECT id, label, created_at, last_used_at FROM api_keys ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?)
}

pub async fn revoke(pool: &SqlitePool, id: &str) -> Result<(), ApiKeyError> {
    sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_last_used(pool: &SqlitePool, id: &str) -> Result<(), ApiKeyError> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE api_keys SET last_used_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool() -> SqlitePool {
        db::init("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn generate_and_verify() {
        let pool = test_pool().await;
        let plaintext = generate(&pool, Some("test key")).await.unwrap();
        assert!(plaintext.len() > 20);
        let key_id = verify_key(&pool, &plaintext).await.unwrap();
        assert!(key_id.is_some());
    }

    #[tokio::test]
    async fn wrong_key_rejected() {
        let pool = test_pool().await;
        generate(&pool, None).await.unwrap();
        let result = verify_key(&pool, "wrong-key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn revoke_removes_key() {
        let pool = test_pool().await;
        let plaintext = generate(&pool, Some("to revoke")).await.unwrap();
        let keys = list(&pool).await.unwrap();
        revoke(&pool, &keys[0].id).await.unwrap();
        let result = verify_key(&pool, &plaintext).await.unwrap();
        assert!(result.is_none());
    }
}

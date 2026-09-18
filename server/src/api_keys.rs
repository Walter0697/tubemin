use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{Duration, Utc};
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ApiKeyOwner<'a> {
    pub sub: Option<&'a str>,
    pub display: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSetupOwner {
    pub sub: Option<String>,
    pub display: String,
}

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
    pub owner_sub: Option<String>,
    pub owner_display: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VerifiedApiKey {
    pub id: String,
    pub label: Option<String>,
    pub owner_sub: Option<String>,
    pub owner_display: Option<String>,
}

pub async fn generate(
    pool: &SqlitePool,
    label: Option<&str>,
    owner: ApiKeyOwner<'_>,
) -> Result<String, ApiKeyError> {
    let plaintext = Uuid::new_v4().to_string() + "-" + &Uuid::new_v4().to_string();
    let key_hash = hash(&plaintext, DEFAULT_COST)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO api_keys (id, key_hash, label, owner_sub, owner_display, created_at) VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&key_hash)
    .bind(label)
    .bind(owner.sub)
    .bind(owner.display)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(plaintext)
}

pub async fn verify_key(
    pool: &SqlitePool,
    plaintext: &str,
) -> Result<Option<VerifiedApiKey>, ApiKeyError> {
    let keys = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, Option<String>)>(
        "SELECT id, key_hash, label, owner_sub, owner_display FROM api_keys",
    )
    .fetch_all(pool)
    .await?;

    let plaintext = plaintext.to_string();
    let result = tokio::task::spawn_blocking(move || {
        for (id, key_hash, label, owner_sub, owner_display) in keys {
            if verify(&plaintext, &key_hash).unwrap_or(false) {
                return Some(VerifiedApiKey {
                    id,
                    label,
                    owner_sub,
                    owner_display,
                });
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
        "SELECT id, label, owner_sub, owner_display, created_at, last_used_at FROM api_keys ORDER BY created_at DESC"
    )
    .fetch_all(pool)
    .await?)
}

pub async fn list_by_owner(
    pool: &SqlitePool,
    owner_sub: Option<&str>,
    owner_display: &str,
) -> Result<Vec<ApiKey>, ApiKeyError> {
    let query = if owner_sub.is_some() {
        "SELECT id, label, owner_sub, owner_display, created_at, last_used_at FROM api_keys WHERE owner_sub = ? ORDER BY created_at DESC"
    } else {
        "SELECT id, label, owner_sub, owner_display, created_at, last_used_at FROM api_keys WHERE owner_sub IS NULL AND owner_display = ? ORDER BY created_at DESC"
    };
    Ok(sqlx::query_as::<_, ApiKey>(query)
        .bind(owner_sub.unwrap_or(owner_display))
        .fetch_all(pool)
        .await?)
}

pub async fn revoke_by_owner(
    pool: &SqlitePool,
    id: &str,
    owner_sub: Option<&str>,
    owner_display: &str,
) -> Result<bool, ApiKeyError> {
    let query = if owner_sub.is_some() {
        "DELETE FROM api_keys WHERE id = ? AND owner_sub = ?"
    } else {
        "DELETE FROM api_keys WHERE id = ? AND owner_sub IS NULL AND owner_display = ?"
    };
    let result = sqlx::query(query)
        .bind(id)
        .bind(owner_sub.unwrap_or(owner_display))
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<ApiKey>, ApiKeyError> {
    Ok(sqlx::query_as::<_, ApiKey>(
        "SELECT id, label, owner_sub, owner_display, created_at, last_used_at FROM api_keys WHERE id = ?"
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .next())
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

pub async fn create_shortcut_setup_token(
    pool: &SqlitePool,
    owner: ApiKeyOwner<'_>,
) -> Result<String, ApiKeyError> {
    let plaintext = Uuid::new_v4().to_string() + "-" + &Uuid::new_v4().to_string();
    let token_hash = hash(&plaintext, DEFAULT_COST)?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let expires_at = (Utc::now() + Duration::minutes(10)).timestamp();

    sqlx::query(
        "INSERT INTO shortcut_setup_tokens (id, token_hash, owner_sub, owner_display, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(token_hash)
    .bind(owner.sub)
    .bind(owner.display)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(plaintext)
}

pub async fn consume_shortcut_setup_token(
    pool: &SqlitePool,
    plaintext: &str,
) -> Result<Option<ShortcutSetupOwner>, ApiKeyError> {
    let now = Utc::now().timestamp();
    let candidates = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        "SELECT id, token_hash, owner_sub, owner_display FROM shortcut_setup_tokens WHERE expires_at > ?",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    let Some((id, _token_hash, owner_sub, owner_display)) = candidates
        .into_iter()
        .find(|(_, token_hash, _, _)| verify(plaintext, token_hash).unwrap_or(false))
    else {
        return Ok(None);
    };

    let deleted = sqlx::query(
        "DELETE FROM shortcut_setup_tokens WHERE id = ? AND expires_at > ?",
    )
    .bind(id)
    .bind(now)
    .execute(pool)
    .await?;

    if deleted.rows_affected() != 1 {
        return Ok(None);
    }

    Ok(Some(ShortcutSetupOwner {
        sub: owner_sub,
        display: owner_display,
    }))
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
        let plaintext = generate(
            &pool,
            Some("test key"),
            ApiKeyOwner {
                sub: Some("user-1"),
                display: "Walter",
            },
        )
        .await
        .unwrap();
        assert!(plaintext.len() > 20);
        let key_id = verify_key(&pool, &plaintext).await.unwrap();
        assert!(key_id.is_some());
        let verified = key_id.unwrap();
        assert_eq!(verified.owner_sub.as_deref(), Some("user-1"));
        assert_eq!(verified.owner_display.as_deref(), Some("Walter"));
    }

    #[tokio::test]
    async fn wrong_key_rejected() {
        let pool = test_pool().await;
        generate(
            &pool,
            None,
            ApiKeyOwner {
                sub: None,
                display: "admin",
            },
        )
        .await
        .unwrap();
        let result = verify_key(&pool, "wrong-key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn revoke_removes_key() {
        let pool = test_pool().await;
        let plaintext = generate(
            &pool,
            Some("to revoke"),
            ApiKeyOwner {
                sub: Some("user-1"),
                display: "Walter",
            },
        )
        .await
        .unwrap();
        let keys = list(&pool).await.unwrap();
        revoke(&pool, &keys[0].id).await.unwrap();
        let result = verify_key(&pool, &plaintext).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_by_owner_only_returns_matching_keys() {
        let pool = test_pool().await;
        generate(
            &pool,
            Some("mine"),
            ApiKeyOwner {
                sub: Some("user-1"),
                display: "Walter",
            },
        )
        .await
        .unwrap();
        generate(
            &pool,
            Some("other"),
            ApiKeyOwner {
                sub: Some("user-2"),
                display: "Alice",
            },
        )
        .await
        .unwrap();
        let keys = list_by_owner(&pool, Some("user-1"), "Walter")
            .await
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].label.as_deref(), Some("mine"));
    }

    #[tokio::test]
    async fn shortcut_setup_token_can_be_consumed_only_once() {
        let pool = test_pool().await;
        let token = create_shortcut_setup_token(
            &pool,
            ApiKeyOwner {
                sub: Some("user-1"),
                display: "Walter",
            },
        )
        .await
        .unwrap();

        let owner = consume_shortcut_setup_token(&pool, &token).await.unwrap();
        assert_eq!(owner.unwrap().display, "Walter");
        assert!(consume_shortcut_setup_token(&pool, &token)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn expired_shortcut_setup_token_is_rejected() {
        let pool = test_pool().await;
        let token = create_shortcut_setup_token(
            &pool,
            ApiKeyOwner {
                sub: None,
                display: "Walter",
            },
        )
        .await
        .unwrap();

        sqlx::query("UPDATE shortcut_setup_tokens SET expires_at = 0")
            .execute(&pool)
            .await
            .unwrap();

        assert!(consume_shortcut_setup_token(&pool, &token)
            .await
            .unwrap()
            .is_none());
    }
}

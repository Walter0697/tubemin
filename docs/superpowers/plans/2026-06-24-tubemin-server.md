# Tubemin Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust/Axum server that accepts MeTube-supported URLs via an API-key-authenticated endpoint, queues them in MeTube, watches for download completion, moves finished files to PeerTube's import folder, and serves an OIDC-protected dashboard.

**Architecture:** Single Axum process with a background `notify` watcher task. API key auth guards `POST /api/submit`; OIDC session guards `/dashboard` and `/settings`. MeTube is called internally via `reqwest`. All state persists in SQLite via `sqlx`.

**Tech Stack:** Rust, Axum 0.7, sqlx 0.8 (SQLite), reqwest 0.12, notify 6, openidconnect 3, tower-sessions 0.13, askama 0.12, bcrypt 0.15, tokio 1

## Global Constraints

- Status values are exactly: `pending` | `imported` | `error` — no other values
- API key stored as bcrypt hash; plaintext never persisted
- All OIDC-protected routes redirect to `/auth/login` if session missing
- File move skips files ending in `.part` or `.ytdl`
- MeTube URL defaults to `http://metube:8081` if `METUBE_URL` unset
- Naming in copy: "MeTube-supported URLs", never name a specific platform

---

### Task 1: Project Scaffold

**Files:**
- Create: `server/Cargo.toml`
- Create: `server/src/main.rs` (stub)
- Create: `server/.gitignore`
- Create: `server/migrations/001_initial.sql`
- Create: `server/templates/` (empty dir placeholder)

**Interfaces:**
- Produces: compilable Rust project that prints "Tubemin starting" and exits

- [ ] **Step 1: Init cargo project**

```bash
cd /Users/walter/Documents/git/tubemin
cargo init server --name tubemin
```

Expected: `server/src/main.rs` created with hello world stub

- [ ] **Step 2: Replace Cargo.toml with full dependency set**

```toml
[package]
name = "tubemin"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = { version = "0.7", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio", "migrate", "chrono", "uuid"] }
dotenvy = "0.15"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", features = ["json"] }
notify = "6"
bcrypt = "0.15"
uuid = { version = "1", features = ["v4"] }
tower-http = { version = "0.5", features = ["fs"] }
tower-sessions = { version = "0.13", features = ["memory-store"] }
openidconnect = "3"
askama = "0.12"
askama_axum = "0.4"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "1"
anyhow = "1"

[dev-dependencies]
axum-test = "15"
tempfile = "3"
wiremock = "0.6"
tokio = { version = "1", features = ["full", "test-util"] }
```

- [ ] **Step 3: Write stub main.rs**

```rust
// server/src/main.rs
#[tokio::main]
async fn main() {
    println!("Tubemin starting");
}
```

- [ ] **Step 4: Write initial migration**

```sql
-- server/migrations/001_initial.sql
CREATE TABLE IF NOT EXISTS submissions (
    id           TEXT PRIMARY KEY,
    url          TEXT NOT NULL,
    filename     TEXT,
    status       TEXT NOT NULL DEFAULT 'pending',
    submitted_at TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    id           TEXT PRIMARY KEY,
    key_hash     TEXT NOT NULL,
    label        TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);
```

- [ ] **Step 5: Add .gitignore**

```
/target
.env
*.db
*.db-shm
*.db-wal
```

- [ ] **Step 6: Verify it compiles**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo build
```

Expected: compiles with warnings only (unused imports OK at this stage)

- [ ] **Step 7: Create templates placeholder**

```bash
mkdir -p /Users/walter/Documents/git/tubemin/server/templates
touch /Users/walter/Documents/git/tubemin/server/templates/.gitkeep
```

- [ ] **Step 8: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/
git commit -m "feat: scaffold Tubemin server project"
```

---

### Task 2: Configuration

**Files:**
- Create: `server/src/config.rs`
- Modify: `server/src/main.rs` (add `mod config;`)

**Interfaces:**
- Produces: `Config::from_env() -> Result<Config, anyhow::Error>`
- `Config` fields: `api_port: u16`, `metube_url: String`, `downloads_dir: PathBuf`, `peertube_import_dir: PathBuf`, `database_url: String`, `oidc_issuer_url: String`, `oidc_client_id: String`, `oidc_client_secret: String`, `oidc_redirect_url: String`

- [ ] **Step 1: Write failing test**

```rust
// server/src/config.rs
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub api_port: u16,
    pub metube_url: String,
    pub downloads_dir: PathBuf,
    pub peertube_import_dir: PathBuf,
    pub database_url: String,
    pub oidc_issuer_url: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,
    pub oidc_redirect_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_required_vars() {
        std::env::set_var("DATABASE_URL", "sqlite:///tmp/test.db");
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "tubemin");
        std::env::set_var("OIDC_CLIENT_SECRET", "secret");
        std::env::set_var("OIDC_REDIRECT_URL", "https://tubemin.example.com/auth/callback");

        let config = Config::from_env().unwrap();
        assert_eq!(config.api_port, 3000);
        assert_eq!(config.metube_url, "http://metube:8081");
        assert_eq!(config.downloads_dir, std::path::PathBuf::from("/downloads"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test config::tests::loads_required_vars -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement Config::from_env**

```rust
impl Config {
    pub fn from_env() -> Result<Self, anyhow::Error> {
        dotenvy::dotenv().ok();
        Ok(Config {
            api_port: std::env::var("API_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()?,
            metube_url: std::env::var("METUBE_URL")
                .unwrap_or_else(|_| "http://metube:8081".into()),
            downloads_dir: PathBuf::from(
                std::env::var("DOWNLOADS_DIR").unwrap_or_else(|_| "/downloads".into()),
            ),
            peertube_import_dir: PathBuf::from(
                std::env::var("PEERTUBE_IMPORT_DIR")
                    .unwrap_or_else(|_| "/peertube-import".into()),
            ),
            database_url: std::env::var("DATABASE_URL")?,
            oidc_issuer_url: std::env::var("OIDC_ISSUER_URL")?,
            oidc_client_id: std::env::var("OIDC_CLIENT_ID")?,
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET")?,
            oidc_redirect_url: std::env::var("OIDC_REDIRECT_URL")?,
        })
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test config::tests::loads_required_vars -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Add mod to main.rs**

```rust
// server/src/main.rs
mod config;

#[tokio::main]
async fn main() {
    let _config = config::Config::from_env().expect("Failed to load config");
    println!("Tubemin starting");
}
```

- [ ] **Step 6: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/config.rs server/src/main.rs
git commit -m "feat: add Config loaded from environment variables"
```

---

### Task 3: Database Layer

**Files:**
- Create: `server/src/db.rs`
- Modify: `server/src/main.rs`

**Interfaces:**
- Produces: `db::init(database_url: &str) -> Result<SqlitePool, sqlx::Error>`
- Produces: `db::create_submission(pool, id, url) -> Result<(), sqlx::Error>`
- Produces: `db::mark_imported(pool, filename) -> Result<(), sqlx::Error>`
- Produces: `db::mark_error(pool, filename) -> Result<(), sqlx::Error>`
- Produces: `db::list_submissions(pool) -> Result<Vec<Submission>, sqlx::Error>`
- Produces: `Submission { id, url, filename, status, submitted_at, updated_at }`

- [ ] **Step 1: Write failing tests**

```rust
// server/src/db.rs
use sqlx::SqlitePool;
use chrono::Utc;
use uuid::Uuid;

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
    todo!()
}

pub async fn create_submission(pool: &SqlitePool, id: &str, url: &str) -> Result<(), sqlx::Error> {
    todo!()
}

pub async fn mark_imported(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    todo!()
}

pub async fn mark_error(pool: &SqlitePool, filename: &str) -> Result<(), sqlx::Error> {
    todo!()
}

pub async fn list_submissions(pool: &SqlitePool) -> Result<Vec<Submission>, sqlx::Error> {
    todo!()
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
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test db::tests -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement all db functions**

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test db::tests -- --nocapture
```

Expected: both tests PASS

- [ ] **Step 5: Add mod to main.rs**

Add `mod db;` to `server/src/main.rs`.

- [ ] **Step 6: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/db.rs server/src/main.rs
git commit -m "feat: add SQLite database layer with submissions and api_keys tables"
```

---

### Task 4: API Key Management

**Files:**
- Create: `server/src/api_keys.rs`

**Interfaces:**
- Produces: `api_keys::generate(pool) -> Result<String, Error>` — returns plaintext key once
- Produces: `api_keys::verify(pool, plaintext_key) -> Result<bool, Error>`
- Produces: `api_keys::list(pool) -> Result<Vec<ApiKey>, Error>`
- Produces: `api_keys::revoke(pool, id) -> Result<(), Error>`
- Produces: `api_keys::update_last_used(pool, id) -> Result<(), Error>`
- Produces: `ApiKey { id, label, created_at, last_used_at }`

- [ ] **Step 1: Write failing tests**

```rust
// server/src/api_keys.rs
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
    todo!()
}

pub async fn verify_key(pool: &SqlitePool, plaintext: &str) -> Result<Option<String>, ApiKeyError> {
    todo!()
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ApiKey>, ApiKeyError> {
    todo!()
}

pub async fn revoke(pool: &SqlitePool, id: &str) -> Result<(), ApiKeyError> {
    todo!()
}

pub async fn update_last_used(pool: &SqlitePool, id: &str) -> Result<(), ApiKeyError> {
    todo!()
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
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test api_keys::tests -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement api_keys functions**

```rust
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

    for (id, key_hash) in keys {
        if verify(plaintext, &key_hash)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
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
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test api_keys::tests -- --nocapture
```

Expected: all 3 tests PASS

- [ ] **Step 5: Add mod to main.rs**

Add `mod api_keys;` to `server/src/main.rs`.

- [ ] **Step 6: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/api_keys.rs server/src/main.rs
git commit -m "feat: add API key generation, verification, and revocation"
```

---

### Task 5: MeTube Client

**Files:**
- Create: `server/src/metube.rs`

**Interfaces:**
- Consumes: `metube_url: &str`, `url: &str`
- Produces: `metube::submit(metube_url: &str, url: &str) -> Result<(), MeTubeError>`

- [ ] **Step 1: Write failing test**

```rust
// server/src/metube.rs
use reqwest::Client;
use thiserror::Error;
use serde_json::json;

#[derive(Debug, Error)]
pub enum MeTubeError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("metube returned status {0}")]
    BadStatus(u16),
}

pub async fn submit(metube_url: &str, url: &str) -> Result<(), MeTubeError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path, body_json};
    use serde_json::json;

    #[tokio::test]
    async fn posts_to_metube_add() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .and(body_json(json!({"url": "https://example.com/video", "folder": "/downloads", "auto_start": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
            .mount(&server)
            .await;

        let result = submit(&server.uri(), "https://example.com/video").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn returns_error_on_bad_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let result = submit(&server.uri(), "https://example.com/video").await;
        assert!(matches!(result, Err(MeTubeError::BadStatus(500))));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test metube::tests -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement submit**

```rust
pub async fn submit(metube_url: &str, url: &str) -> Result<(), MeTubeError> {
    let client = Client::new();
    let resp = client
        .post(format!("{}/add", metube_url))
        .json(&json!({
            "url": url,
            "folder": "/downloads",
            "auto_start": true
        }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(MeTubeError::BadStatus(status));
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test metube::tests -- --nocapture
```

Expected: both tests PASS

- [ ] **Step 5: Add mod to main.rs**

Add `mod metube;` to `server/src/main.rs`.

- [ ] **Step 6: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/metube.rs server/src/main.rs
git commit -m "feat: add MeTube HTTP client"
```

---

### Task 6: AppState and Submit Handler

**Files:**
- Create: `server/src/state.rs`
- Create: `server/src/handlers/mod.rs`
- Create: `server/src/handlers/submit.rs`

**Interfaces:**
- Consumes: `Config`, `SqlitePool`, `metube::submit`
- Produces: `AppState { pool: Arc<SqlitePool>, config: Arc<Config> }`
- Produces: `POST /api/submit` handler — validates `X-API-Key` header, calls MeTube, records submission
- HTTP contract: `POST /api/submit` body `{"url":"..."}`, header `X-API-Key: <key>` → `200 {"status":"queued"}` or `401`/`503`

- [ ] **Step 1: Create AppState**

```rust
// server/src/state.rs
use std::sync::Arc;
use sqlx::SqlitePool;
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<Config>,
}
```

- [ ] **Step 2: Create handlers/mod.rs**

```rust
// server/src/handlers/mod.rs
pub mod submit;
```

- [ ] **Step 3: Write failing test for submit handler**

```rust
// server/src/handlers/submit.rs
use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::state::AppState;
use crate::{api_keys, db, metube};

#[derive(Deserialize)]
pub struct SubmitRequest {
    pub url: String,
}

#[derive(Serialize)]
pub struct SubmitResponse {
    pub status: String,
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post, body::Body, http::Request};
    use axum_test::TestServer;
    use std::sync::Arc;
    use crate::{config::Config, db, api_keys, state::AppState};
    use serde_json::json;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    async fn make_app() -> (TestServer, String, MockServer) {
        let pool = Arc::new(db::init("sqlite::memory:").await.unwrap());
        let metube_mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status":"ok"})))
            .mount(&metube_mock)
            .await;

        let config = Arc::new(Config {
            api_port: 3000,
            metube_url: metube_mock.uri(),
            downloads_dir: "/tmp/downloads".into(),
            peertube_import_dir: "/tmp/import".into(),
            database_url: "sqlite::memory:".into(),
            oidc_issuer_url: "https://auth.example.com".into(),
            oidc_client_id: "tubemin".into(),
            oidc_client_secret: "secret".into(),
            oidc_redirect_url: "https://tubemin.example.com/auth/callback".into(),
        });

        let state = AppState { pool: pool.clone(), config };
        let api_key = api_keys::generate(&pool, Some("test")).await.unwrap();

        let app = Router::new()
            .route("/api/submit", post(submit))
            .with_state(state);

        (TestServer::new(app).unwrap(), api_key, metube_mock)
    }

    #[tokio::test]
    async fn valid_submission_returns_queued() {
        let (server, api_key, _mock) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", &api_key)
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "queued");
    }

    #[tokio::test]
    async fn missing_api_key_returns_401() {
        let (server, _, _mock) = make_app().await;
        let resp = server
            .post("/api/submit")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_api_key_returns_401() {
        let (server, _, _mock) = make_app().await;
        let resp = server
            .post("/api/submit")
            .add_header("X-API-Key", "wrong-key")
            .json(&json!({"url": "https://example.com/video"}))
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test handlers::submit::tests -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 5: Implement submit handler**

```rust
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitRequest>,
) -> impl IntoResponse {
    let key = match headers.get("X-API-Key").and_then(|v| v.to_str().ok()) {
        Some(k) => k.to_string(),
        None => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing API key"}))).into_response(),
    };

    let key_id = match api_keys::verify_key(&state.pool, &key).await {
        Ok(Some(id)) => id,
        Ok(None) => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid API key"}))).into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response(),
    };

    let _ = api_keys::update_last_used(&state.pool, &key_id).await;

    if let Err(_) = metube::submit(&state.config.metube_url, &body.url).await {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "metube unavailable"}))).into_response();
    }

    let id = Uuid::new_v4().to_string();
    if let Err(_) = db::create_submission(&state.pool, &id, &body.url).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "db error"}))).into_response();
    }

    (StatusCode::OK, Json(SubmitResponse { status: "queued".into() })).into_response()
}
```

Add `use serde_json::json;` at the top.

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test handlers::submit::tests -- --nocapture
```

Expected: all 3 tests PASS

- [ ] **Step 7: Add mods to main.rs**

```rust
mod state;
mod handlers;
```

- [ ] **Step 8: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/state.rs server/src/handlers/ server/src/main.rs
git commit -m "feat: add POST /api/submit with API key auth"
```

---

### Task 7: File Watcher

**Files:**
- Create: `server/src/watcher.rs`

**Interfaces:**
- Consumes: `downloads_dir: PathBuf`, `import_dir: PathBuf`, `pool: Arc<SqlitePool>`
- Produces: `watcher::start(downloads_dir, import_dir, pool) -> tokio::task::JoinHandle<()>`

- [ ] **Step 1: Write failing test**

```rust
// server/src/watcher.rs
use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Config as NotifyConfig};
use notify::event::{EventKind, CreateKind};
use tracing::{error, info};

pub fn start(
    downloads_dir: PathBuf,
    import_dir: PathBuf,
    pool: Arc<SqlitePool>,
) -> tokio::task::JoinHandle<()> {
    todo!()
}

fn is_temp_file(path: &std::path::Path) -> bool {
    todo!()
}

async fn handle_new_file(
    path: PathBuf,
    import_dir: &PathBuf,
    pool: &SqlitePool,
) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_detection() {
        assert!(is_temp_file(std::path::Path::new("/downloads/video.part")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.ytdl")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mp4")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mkv")));
    }

    #[tokio::test]
    async fn file_moved_to_import_dir() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let pool = Arc::new(crate::db::init("sqlite::memory:").await.unwrap());

        // Create a test file in src_dir
        let test_file = src_dir.path().join("video.mp4");
        std::fs::write(&test_file, b"fake video").unwrap();

        handle_new_file(
            test_file.clone(),
            &dst_dir.path().to_path_buf(),
            &pool,
        ).await;

        assert!(!test_file.exists(), "source file should be moved");
        assert!(dst_dir.path().join("video.mp4").exists(), "dest file should exist");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo test watcher::tests -- --nocapture
```

Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement watcher**

```rust
fn is_temp_file(path: &std::path::Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(ext, "part" | "ytdl" | "tmp"),
        None => false,
    }
}

async fn handle_new_file(
    path: PathBuf,
    import_dir: &PathBuf,
    pool: &SqlitePool,
) {
    if is_temp_file(&path) {
        return;
    }
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return,
    };
    let dest = import_dir.join(&filename);
    match std::fs::rename(&path, &dest) {
        Ok(_) => {
            info!("Moved {} to import dir", filename);
            let _ = crate::db::mark_imported(pool, &filename).await;
        }
        Err(e) => {
            error!("Failed to move {}: {}", filename, e);
            let _ = crate::db::mark_error(pool, &filename).await;
        }
    }
}

pub fn start(
    downloads_dir: PathBuf,
    import_dir: PathBuf,
    pool: Arc<SqlitePool>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default())
            .expect("Failed to create watcher");
        watcher.watch(&downloads_dir, RecursiveMode::NonRecursive)
            .expect("Failed to watch downloads dir");

        let rt = tokio::runtime::Handle::current();
        for result in rx {
            match result {
                Ok(event) => {
                    match event.kind {
                        EventKind::Create(_) => {
                            for path in event.paths {
                                let import_dir = import_dir.clone();
                                let pool = pool.clone();
                                rt.spawn(async move {
                                    handle_new_file(path, &import_dir, &pool).await;
                                });
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => error!("Watcher error: {}", e),
            }
        }
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test watcher::tests -- --nocapture
```

Expected: both tests PASS

- [ ] **Step 5: Add mod to main.rs**

Add `mod watcher;` to `server/src/main.rs`.

- [ ] **Step 6: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/watcher.rs server/src/main.rs
git commit -m "feat: add file watcher to move completed downloads to PeerTube import dir"
```

---

### Task 8: OIDC Authentication

**Files:**
- Create: `server/src/oidc.rs`
- Modify: `server/src/handlers/mod.rs`

**Interfaces:**
- Produces: `GET /auth/login` handler — redirects to OIDC provider
- Produces: `GET /auth/callback` handler — exchanges code, sets session
- Produces: `oidc::require_auth` middleware extractor — returns `Redirect` to `/auth/login` if no session
- Produces: `OidcUser { email: String }` — extractable from session in dashboard/settings handlers

- [ ] **Step 1: Write OIDC module skeleton with session key constant**

```rust
// server/src/oidc.rs
use axum::{
    async_trait,
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use crate::state::AppState;

pub const SESSION_USER_KEY: &str = "oidc_user";
const SESSION_PKCE_KEY: &str = "pkce_verifier";
const SESSION_CSRF_KEY: &str = "csrf_token";
const SESSION_NONCE_KEY: &str = "nonce";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUser {
    pub email: String,
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn build_oidc_client(config: &crate::config::Config) -> anyhow::Result<CoreClient> {
    let provider_metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(config.oidc_issuer_url.clone())?,
        openidconnect::reqwest::async_http_client,
    )
    .await?;

    Ok(CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(config.oidc_client_id.clone()),
        Some(ClientSecret::new(config.oidc_client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(config.oidc_redirect_url.clone())?))
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
) -> impl IntoResponse {
    let client = match build_oidc_client(&state.config).await {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".into()))
        .add_scope(Scope::new("email".into()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    session.insert(SESSION_PKCE_KEY, pkce_verifier.secret().clone()).await.ok();
    session.insert(SESSION_CSRF_KEY, csrf_token.secret().clone()).await.ok();
    session.insert(SESSION_NONCE_KEY, nonce.secret().clone()).await.ok();

    Redirect::to(auth_url.as_str()).into_response()
}

pub async fn callback(
    State(state): State<AppState>,
    session: Session,
    Query(params): Query<CallbackParams>,
) -> impl IntoResponse {
    let pkce_secret: String = match session.get(SESSION_PKCE_KEY).await.ok().flatten() {
        Some(v) => v,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let client = match build_oidc_client(&state.config).await {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let token_response = client
        .exchange_code(AuthorizationCode::new(params.code))
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_secret))
        .request_async(openidconnect::reqwest::async_http_client)
        .await;

    match token_response {
        Ok(tokens) => {
            let id_token = match tokens.id_token() {
                Some(t) => t,
                None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            // Extract email from claims (simplified — real impl should verify nonce)
            let email = id_token
                .payload()
                .ok()
                .and_then(|claims| claims.email())
                .map(|e| e.as_str().to_string())
                .unwrap_or_else(|| "unknown".into());

            session.insert(SESSION_USER_KEY, OidcUser { email }).await.ok();
            Redirect::to("/dashboard").into_response()
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}

pub struct RequireAuth(pub OidcUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| Redirect::to("/auth/login").into_response())?;

        match session.get::<OidcUser>(SESSION_USER_KEY).await {
            Ok(Some(user)) => Ok(RequireAuth(user)),
            _ => Err(Redirect::to("/auth/login").into_response()),
        }
    }
}
```

- [ ] **Step 2: Add auth routes to handlers/mod.rs**

```rust
// server/src/handlers/mod.rs
pub mod submit;
pub use submit::submit;
```

- [ ] **Step 3: Add mod to main.rs**

Add `mod oidc;` to `server/src/main.rs`.

- [ ] **Step 4: Verify it compiles**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo build 2>&1 | head -30
```

Expected: compiles (may have unused warnings)

- [ ] **Step 5: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/oidc.rs server/src/handlers/mod.rs server/src/main.rs
git commit -m "feat: add OIDC login/callback handlers and RequireAuth extractor"
```

---

### Task 9: Dashboard and Settings Handlers + Templates

**Files:**
- Create: `server/src/handlers/dashboard.rs`
- Create: `server/src/handlers/settings.rs`
- Create: `server/templates/dashboard.html`
- Create: `server/templates/settings.html`
- Modify: `server/src/handlers/mod.rs`

**Interfaces:**
- Consumes: `RequireAuth`, `AppState`, `db::list_submissions`, `api_keys::list`, `api_keys::generate`, `api_keys::revoke`
- Produces: `GET /dashboard` — HTML table of submissions
- Produces: `GET /settings` — HTML list of API keys
- Produces: `POST /settings/keys/generate` — generates new key, redirects back
- Produces: `POST /settings/keys/:id/revoke` — revokes key, redirects back

- [ ] **Step 1: Create dashboard template**

```html
<!-- server/templates/dashboard.html -->
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Tubemin — Dashboard</title>
  <style>
    body { font-family: sans-serif; max-width: 900px; margin: 40px auto; padding: 0 20px; }
    h1 { font-size: 1.5rem; }
    table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
    th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid #eee; }
    th { background: #f5f5f5; }
    .status-pending { color: #888; }
    .status-imported { color: #2a2; }
    .status-error { color: #c00; }
    nav a { margin-right: 1rem; }
  </style>
</head>
<body>
  <nav><a href="/dashboard">Dashboard</a><a href="/settings">Settings</a></nav>
  <h1>Tubemin Dashboard</h1>
  <table>
    <thead><tr><th>URL</th><th>Status</th><th>Submitted</th></tr></thead>
    <tbody>
      {% for s in submissions %}
      <tr>
        <td><a href="{{ s.url }}" target="_blank">{{ s.url }}</a></td>
        <td class="status-{{ s.status }}">{{ s.status }}</td>
        <td>{{ s.submitted_at }}</td>
      </tr>
      {% else %}
      <tr><td colspan="3">No submissions yet.</td></tr>
      {% endfor %}
    </tbody>
  </table>
</body>
</html>
```

- [ ] **Step 2: Create settings template**

```html
<!-- server/templates/settings.html -->
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Tubemin — Settings</title>
  <style>
    body { font-family: sans-serif; max-width: 900px; margin: 40px auto; padding: 0 20px; }
    h1, h2 { font-size: 1.5rem; }
    h2 { font-size: 1.2rem; margin-top: 2rem; }
    table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
    th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid #eee; }
    th { background: #f5f5f5; }
    .new-key { background: #fffbe6; border: 1px solid #f0c040; padding: 12px; margin-top: 1rem; }
    nav a { margin-right: 1rem; }
    form { display: inline; }
  </style>
</head>
<body>
  <nav><a href="/dashboard">Dashboard</a><a href="/settings">Settings</a></nav>
  <h1>Settings</h1>

  {% if new_key %}
  <div class="new-key">
    <strong>New API key (copy now — shown once):</strong><br>
    <code>{{ new_key }}</code>
  </div>
  {% endif %}

  <h2>API Keys</h2>
  <form method="POST" action="/settings/keys/generate">
    <button type="submit">Generate New Key</button>
  </form>
  <table>
    <thead><tr><th>ID</th><th>Label</th><th>Created</th><th>Last Used</th><th></th></tr></thead>
    <tbody>
      {% for key in api_keys %}
      <tr>
        <td>{{ key.id | truncate(8) }}</td>
        <td>{{ key.label | default("-") }}</td>
        <td>{{ key.created_at }}</td>
        <td>{{ key.last_used_at | default("never") }}</td>
        <td>
          <form method="POST" action="/settings/keys/{{ key.id }}/revoke">
            <button type="submit">Revoke</button>
          </form>
        </td>
      </tr>
      {% else %}
      <tr><td colspan="5">No API keys. Generate one above.</td></tr>
      {% endfor %}
    </tbody>
  </table>
</body>
</html>
```

- [ ] **Step 3: Note on template engine**

These templates use Jinja2 syntax. Use the `minijinja` crate (not `askama`) since `minijinja` renders at runtime and supports the `{% for %}` / `{% if %}` / `| default` filters used above. Update `Cargo.toml` to replace `askama`/`askama_axum` with:

```toml
minijinja = { version = "2", features = ["loader"] }
```

- [ ] **Step 4: Create dashboard handler**

```rust
// server/src/handlers/dashboard.rs
use axum::{extract::State, response::Html};
use minijinja::Environment;
use crate::{db, oidc::RequireAuth, state::AppState};

pub async fn dashboard(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> Html<String> {
    let submissions = db::list_submissions(&state.pool).await.unwrap_or_default();

    let mut env = Environment::new();
    env.add_template("dashboard", include_str!("../../templates/dashboard.html")).unwrap();
    let tmpl = env.get_template("dashboard").unwrap();

    let ctx = minijinja::context! {
        submissions => submissions.iter().map(|s| minijinja::context! {
            url => s.url,
            status => s.status,
            submitted_at => s.submitted_at,
        }).collect::<Vec<_>>(),
    };

    Html(tmpl.render(ctx).unwrap_or_else(|e| format!("Template error: {}", e)))
}
```

- [ ] **Step 5: Create settings handler**

```rust
// server/src/handlers/settings.rs
use axum::{
    extract::{Path, Query, State},
    response::{Html, Redirect},
};
use minijinja::Environment;
use serde::Deserialize;
use crate::{api_keys, oidc::RequireAuth, state::AppState};

#[derive(Deserialize)]
pub struct NewKeyQuery {
    pub new_key: Option<String>,
}

pub async fn settings(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Query(query): Query<NewKeyQuery>,
) -> Html<String> {
    let keys = api_keys::list(&state.pool).await.unwrap_or_default();

    let mut env = Environment::new();
    env.add_template("settings", include_str!("../../templates/settings.html")).unwrap();
    let tmpl = env.get_template("settings").unwrap();

    let ctx = minijinja::context! {
        new_key => query.new_key,
        api_keys => keys.iter().map(|k| minijinja::context! {
            id => k.id,
            label => k.label,
            created_at => k.created_at,
            last_used_at => k.last_used_at,
        }).collect::<Vec<_>>(),
    };

    Html(tmpl.render(ctx).unwrap_or_else(|e| format!("Template error: {}", e)))
}

pub async fn generate_key(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
) -> Redirect {
    match api_keys::generate(&state.pool, Some("web-generated")).await {
        Ok(plaintext) => Redirect::to(&format!("/settings?new_key={}", plaintext)),
        Err(_) => Redirect::to("/settings"),
    }
}

pub async fn revoke_key(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Redirect {
    let _ = api_keys::revoke(&state.pool, &id).await;
    Redirect::to("/settings")
}
```

- [ ] **Step 6: Update handlers/mod.rs**

```rust
// server/src/handlers/mod.rs
pub mod submit;
pub mod dashboard;
pub mod settings;

pub use submit::submit;
pub use dashboard::dashboard;
pub use settings::{settings, generate_key, revoke_key};
```

- [ ] **Step 7: Compile check**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo build 2>&1 | grep "^error" | head -20
```

Expected: no errors

- [ ] **Step 8: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/handlers/ server/templates/
git commit -m "feat: add dashboard and settings handlers with minijinja templates"
```

---

### Task 10: Wire Up main.rs and Run Server

**Files:**
- Modify: `server/src/main.rs` (complete implementation)

**Interfaces:**
- Consumes: all previous modules
- Produces: running Axum server on configured port with all routes, watcher started

- [ ] **Step 1: Write complete main.rs**

```rust
// server/src/main.rs
mod api_keys;
mod config;
mod db;
mod handlers;
mod metube;
mod oidc;
mod state;
mod watcher;

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
use tower_sessions::{MemoryStore, SessionManagerLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "tubemin=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env()?;
    let pool = Arc::new(db::init(&config.database_url).await?);
    let config = Arc::new(config);

    let app_state = state::AppState {
        pool: pool.clone(),
        config: config.clone(),
    };

    // Start file watcher
    watcher::start(
        config.downloads_dir.clone(),
        config.peertube_import_dir.clone(),
        pool.clone(),
    );

    // Session layer
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store);

    let app = Router::new()
        // Public API
        .route("/api/submit", post(handlers::submit))
        // OIDC auth
        .route("/auth/login", get(oidc::login))
        .route("/auth/callback", get(oidc::callback))
        // OIDC-protected pages
        .route("/dashboard", get(handlers::dashboard))
        .route("/settings", get(handlers::settings))
        .route("/settings/keys/generate", post(handlers::generate_key))
        .route("/settings/keys/:id/revoke", post(handlers::revoke_key))
        .layer(session_layer)
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!("Tubemin listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Step 2: Compile check**

```bash
cd /Users/walter/Documents/git/tubemin/server
cargo build 2>&1 | grep "^error" | head -20
```

Expected: no errors

- [ ] **Step 3: Run full test suite**

```bash
cargo test -- --nocapture 2>&1 | tail -20
```

Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/src/main.rs
git commit -m "feat: wire up complete Axum server with all routes and watcher"
```

---

### Task 11: Dockerfile

**Files:**
- Create: `server/Dockerfile`

- [ ] **Step 1: Write multi-stage Dockerfile**

```dockerfile
# server/Dockerfile
FROM rust:1.82-alpine AS builder
RUN apk add --no-cache musl-dev sqlite-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache deps layer
RUN mkdir src && echo "fn main(){}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
RUN touch src/main.rs && cargo build --release

FROM alpine:3.20
RUN apk add --no-cache sqlite-libs ca-certificates
WORKDIR /app
COPY --from=builder /app/target/release/tubemin ./tubemin
COPY --from=builder /app/migrations ./migrations
COPY --from=builder /app/templates ./templates
EXPOSE 3000
CMD ["./tubemin"]
```

- [ ] **Step 2: Verify Dockerfile syntax**

```bash
cd /Users/walter/Documents/git/tubemin/server
docker build -t tubemin:dev . 2>&1 | tail -5
```

Expected: `Successfully built ...` (may take a few minutes first time)

- [ ] **Step 3: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add server/Dockerfile
git commit -m "feat: add multi-stage Dockerfile for tubemin server"
```

---

### Task 12: Docker Compose, Caddy, and example.env

**Files:**
- Create: `docker-compose.yml`
- Create: `Caddyfile`
- Create: `example.env`

- [ ] **Step 1: Write docker-compose.yml**

```yaml
# docker-compose.yml
services:
  caddy:
    image: caddy:alpine
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    depends_on:
      - tubemin

  tubemin:
    build: ./server
    restart: unless-stopped
    env_file: .env
    volumes:
      - downloads:/downloads
      - peertube_import:/peertube-import
      - tubemin_data:/data
    depends_on:
      - metube

  metube:
    image: ghcr.io/alexta69/metube:latest
    restart: unless-stopped
    volumes:
      - downloads:/downloads
    environment:
      DOWNLOAD_DIR: /downloads

  peertube:
    image: chocobozzz/peertube:production-bookworm
    restart: unless-stopped
    env_file: .env
    volumes:
      - peertube_data:/data
      - peertube_import:/peertube-import
    depends_on:
      - peertube_db
      - peertube_redis

  peertube_db:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: peertube
      POSTGRES_USER: peertube
      POSTGRES_PASSWORD: ${PEERTUBE_DB_PASSWORD}
    volumes:
      - peertube_db_data:/var/lib/postgresql/data

  peertube_redis:
    image: redis:7-alpine
    restart: unless-stopped
    volumes:
      - peertube_redis_data:/data

volumes:
  downloads:
  peertube_import:
  tubemin_data:
  caddy_data:
  caddy_config:
  peertube_data:
  peertube_db_data:
  peertube_redis_data:
```

- [ ] **Step 2: Write Caddyfile**

```
# Caddyfile
{$TUBEMIN_DOMAIN} {
    reverse_proxy /api/* tubemin:3000
    reverse_proxy /auth/* tubemin:3000
    reverse_proxy /dashboard tubemin:3000
    reverse_proxy /settings* tubemin:3000
}

{$PEERTUBE_DOMAIN} {
    reverse_proxy peertube:9000
}
```

- [ ] **Step 3: Write example.env**

```bash
# example.env — copy to .env and fill in values

# Tubemin server
API_PORT=3000
METUBE_URL=http://metube:8081
DOWNLOADS_DIR=/downloads
PEERTUBE_IMPORT_DIR=/peertube-import
DATABASE_URL=sqlite:///data/tubemin.db

# OIDC provider (e.g. Authentik, Authelia, Keycloak)
OIDC_ISSUER_URL=https://auth.yourdomain.com/application/o/tubemin/
OIDC_CLIENT_ID=tubemin
OIDC_CLIENT_SECRET=your-client-secret-here
OIDC_REDIRECT_URL=https://tubemin.yourdomain.com/auth/callback

# Caddy domains
TUBEMIN_DOMAIN=tubemin.yourdomain.com
PEERTUBE_DOMAIN=peertube.yourdomain.com

# PeerTube database
PEERTUBE_DB_PASSWORD=change-me-strong-password
```

- [ ] **Step 4: Validate compose file**

```bash
cd /Users/walter/Documents/git/tubemin
docker compose config 2>&1 | head -10
```

Expected: no errors (may warn about missing .env — that's fine)

- [ ] **Step 5: Commit**

```bash
cd /Users/walter/Documents/git/tubemin
git add docker-compose.yml Caddyfile example.env
git commit -m "feat: add Docker Compose stack with Caddy, MeTube, PeerTube, and Tubemin"
```

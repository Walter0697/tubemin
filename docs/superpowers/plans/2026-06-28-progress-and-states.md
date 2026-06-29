# Progress & Granular States Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add granular pipeline states (`downloading`, `transcoding`, `complete`) and live progress bars (%) for both ffmpeg direct downloads and MeTube downloads.

**Architecture:** An in-memory `ProgressMap` (`Arc<Mutex<HashMap<String, f32>>>`) lives in `AppState` and is updated by the ffmpeg stdout parser (direct downloads) and the MeTube poller; the submissions API merges it into each row's `progress` field. A new transcoding poller transitions `imported → transcoding → complete` by polling PeerTube's video state endpoint.

**Tech Stack:** Rust/Tokio, SQLite via sqlx, ffmpeg `-progress pipe:1`, MeTube REST `/history`, PeerTube REST `/api/v1/videos/{uuid}`, vanilla JS + CSS in existing templates.

## Global Constraints

- All Rust changes must compile with zero errors (`cargo build --release`).
- All existing tests must continue to pass (`cargo test`).
- No new dependencies — use only crates already in `Cargo.toml`.
- Follow existing code style: no comments unless the WHY is non-obvious, no docstrings.
- DB migrations live in `server/migrations/` numbered sequentially (next is `007`).
- Frontend is plain JS + CSS in `server/templates/dashboard.html` and `server/static/style.css` — no build step.

---

### Task 1: New DB Status Functions + CSS Status Dots

Add `mark_transcoding` / `mark_complete` DB helpers and wire up visual states in the dashboard.

**Files:**
- Modify: `server/src/db.rs`
- Modify: `server/static/style.css`
- Modify: `server/templates/dashboard.html` (filter tabs + `isProcessing` JS)

**Interfaces:**
- Produces:
  - `pub async fn mark_transcoding(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error>`
  - `pub async fn mark_complete(pool: &SqlitePool, peertube_uuid: &str) -> Result<(), sqlx::Error>`
  - CSS classes: `.status-downloading`, `.status-transcoding`, `.status-complete`

- [ ] **Step 1: Write failing tests for new DB functions**

Add to the `#[cfg(test)]` block in `server/src/db.rs`:

```rust
#[tokio::test]
async fn mark_transcoding_transitions_imported() {
    let pool = test_pool().await;
    create_submission(&pool, "t1", "https://example.com/v", None, false).await.unwrap();
    // Simulate imported with a peertube_uuid
    sqlx::query("UPDATE submissions SET status='imported', peertube_uuid='uuid-abc' WHERE id='t1'")
        .execute(&pool).await.unwrap();
    mark_transcoding(&pool, "uuid-abc").await.unwrap();
    let rows = list_submissions(&pool).await.unwrap();
    assert_eq!(rows[0].status, "transcoding");
}

#[tokio::test]
async fn mark_complete_transitions_transcoding() {
    let pool = test_pool().await;
    create_submission(&pool, "t2", "https://example.com/v2", None, false).await.unwrap();
    sqlx::query("UPDATE submissions SET status='transcoding', peertube_uuid='uuid-xyz' WHERE id='t2'")
        .execute(&pool).await.unwrap();
    mark_complete(&pool, "uuid-xyz").await.unwrap();
    let rows = list_submissions(&pool).await.unwrap();
    assert_eq!(rows[0].status, "complete");
}
```

- [ ] **Step 2: Run to confirm they fail**

```bash
cd server && cargo test db::tests::mark_transcoding 2>&1 | tail -5
```
Expected: `error[E0425]: cannot find function 'mark_transcoding'`

- [ ] **Step 3: Implement the two DB functions**

Add to `server/src/db.rs` after `mark_imported_by_url`:

```rust
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
```

- [ ] **Step 4: Run tests**

```bash
cd server && cargo test db::tests 2>&1 | tail -10
```
Expected: all db tests pass.

- [ ] **Step 5: Add CSS for new status dots**

In `server/static/style.css`, find the `.status-error` rule and add below it:

```css
.status-downloading { background: #60a5fa; box-shadow: 0 0 6px #60a5fa66; animation: pulse-blue 1.4s ease-in-out infinite; }
.status-transcoding  { background: var(--status-warn); box-shadow: 0 0 6px #e8884a66; animation: pulse-amber 1.4s ease-in-out infinite; }
.status-complete     { background: #a3e635; box-shadow: 0 0 8px #a3e63566; }

@keyframes pulse-blue {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.45; }
}
@keyframes pulse-amber {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.45; }
}
```

- [ ] **Step 6: Update dashboard filter tabs and `isProcessing`**

In `server/templates/dashboard.html`, replace the filter-tabs div:

```html
<div class="filter-tabs">
  <button class="filter-tab active" data-status="all">All<span class="filter-count" id="count-all"></span></button>
  <button class="filter-tab" data-status="pending">Pending<span class="filter-count" id="count-pending"></span></button>
  <button class="filter-tab" data-status="downloading">Downloading<span class="filter-count" id="count-downloading"></span></button>
  <button class="filter-tab" data-status="transcoding">Transcoding<span class="filter-count" id="count-transcoding"></span></button>
  <button class="filter-tab" data-status="complete">Complete<span class="filter-count" id="count-complete"></span></button>
  <button class="filter-tab" data-status="imported">Imported<span class="filter-count" id="count-imported"></span></button>
  <button class="filter-tab" data-status="error">Error<span class="filter-count" id="count-error"></span></button>
</div>
```

In the `applyData` function, replace the `tabs` array:

```js
const tabs = ['all', 'pending', 'downloading', 'transcoding', 'complete', 'imported', 'error'];
```

Replace the `isProcessing` function:

```js
function isProcessing(status) {
  return status === 'pending' || status === 'downloading' || status === 'transcoding';
}
```

- [ ] **Step 7: Build and confirm no compile errors**

```bash
cd server && cargo build 2>&1 | grep -E "^error" | head -5
```
Expected: no output (no errors).

- [ ] **Step 8: Commit**

```bash
git add server/src/db.rs server/static/style.css server/templates/dashboard.html
git commit -m "feat: add transcoding/complete statuses and status dot styles"
```

---

### Task 2: In-Memory ProgressMap + AppState

Create a shared `ProgressMap` type, wire it into `AppState`, and expose `progress: Option<f32>` on every submission API response.

**Files:**
- Create: `server/src/progress.rs`
- Modify: `server/src/state.rs`
- Modify: `server/src/main.rs`
- Modify: `server/src/handlers/submissions.rs`

**Interfaces:**
- Consumes: nothing from prior tasks
- Produces:
  - `pub type ProgressMap = Arc<std::sync::Mutex<HashMap<String, f32>>>;`
  - `pub fn new_progress_map() -> ProgressMap`
  - `AppState.progress: ProgressMap`
  - `SubmissionRow.progress: Option<f32>`

- [ ] **Step 1: Create `server/src/progress.rs`**

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type ProgressMap = Arc<Mutex<HashMap<String, f32>>>;

pub fn new_progress_map() -> ProgressMap {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn set(map: &ProgressMap, id: &str, pct: f32) {
    if let Ok(mut m) = map.lock() {
        m.insert(id.to_string(), pct.clamp(0.0, 1.0));
    }
}

pub fn remove(map: &ProgressMap, id: &str) {
    if let Ok(mut m) = map.lock() {
        m.remove(id);
    }
}

pub fn get(map: &ProgressMap, id: &str) -> Option<f32> {
    map.lock().ok()?.get(id).copied()
}
```

- [ ] **Step 2: Update `server/src/state.rs`**

```rust
use std::sync::Arc;
use sqlx::SqlitePool;
use crate::config::Config;
use crate::progress::ProgressMap;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub config: Arc<Config>,
    pub progress: ProgressMap,
}
```

- [ ] **Step 3: Update `server/src/main.rs`**

Add `mod progress;` to the module list. In `main()`, after creating `pool`, add:

```rust
let progress_map = progress::new_progress_map();
```

Update the `AppState` construction:

```rust
let app_state = state::AppState {
    pool: pool.clone(),
    config: config.clone(),
    progress: progress_map.clone(),
};
```

- [ ] **Step 4: Add `progress` field to `SubmissionRow` and merge it in handler**

In `server/src/handlers/submissions.rs`, add the field to the struct:

```rust
#[derive(Serialize)]
pub struct SubmissionRow {
    pub id: String,
    pub url: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub peertube_thumb: Option<String>,
    pub peertube_uuid: Option<String>,
    pub status: String,
    pub progress: Option<f32>,
    pub submitted_at: String,
    pub updated_at: String,
}
```

In `list_submissions`, replace the `.map` that builds `SubmissionRow`:

```rust
let submissions = rows.into_iter().map(|s| {
    let progress = crate::progress::get(&state.progress, &s.id);
    SubmissionRow {
        id: s.id,
        url: s.url,
        source_url: s.source_url,
        title: s.title,
        filename: s.filename,
        peertube_thumb: s.peertube_thumb,
        peertube_uuid: s.peertube_uuid,
        status: s.status,
        progress,
        submitted_at: s.submitted_at,
        updated_at: s.updated_at,
    }
}).collect();
```

- [ ] **Step 5: Build and run tests**

```bash
cd server && cargo test 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add server/src/progress.rs server/src/state.rs server/src/main.rs server/src/handlers/submissions.rs
git commit -m "feat: add ProgressMap to AppState and progress field to submissions API"
```

---

### Task 3: ffmpeg Progress Parsing

Capture ffmpeg's live `-progress pipe:1` stdout while also parsing `Duration:` from stderr to compute a 0–1 progress float. Update the ProgressMap in real-time and clean up when the download completes.

**Files:**
- Modify: `server/src/direct_download.rs`
- Modify: `server/src/handlers/submit.rs`

**Interfaces:**
- Consumes: `ProgressMap` from Task 2, `progress::set` / `progress::remove`
- Produces: `download(url, referer, title, cookies, dir, progress_key, progress_map)` signature change

- [ ] **Step 1: Write a test for the progress key cleanup**

Add to `server/src/direct_download.rs` tests:

```rust
#[test]
fn sanitize_removes_null_byte() {
    assert_eq!(sanitize_name("hello\0world"), "hello_world");
}
```

(This confirms we can add tests to this file; the real progress logic is async and integration-tested manually.)

- [ ] **Step 2: Run to confirm it fails**

```bash
cd server && cargo test direct_download::tests::sanitize_removes_null 2>&1 | tail -5
```
Expected: FAIL — `\0` is not currently replaced.

- [ ] **Step 3: Fix `sanitize_name` to also replace null bytes, and update `download_hls` signature**

Replace the `sanitize_name` function:

```rust
fn sanitize_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ => c,
        })
        .collect();
    cleaned.trim().to_string()
}
```

Change the `download` function signature (add two parameters at the end):

```rust
pub async fn download(
    url: &str,
    referer: Option<&str>,
    title: Option<&str>,
    cookies: Option<&str>,
    downloads_dir: &str,
    progress_key: Option<String>,
    progress_map: Option<crate::progress::ProgressMap>,
) -> Result<String, anyhow::Error> {
```

Inside `download`, pass them through to `download_hls`:

```rust
if is_hls {
    download_hls(url, referer, cookies, &dest, progress_key, progress_map).await?;
} else {
    download_direct(url, referer, cookies, &dest).await?;
}
```

- [ ] **Step 4: Update `download_hls` to parse ffmpeg progress**

Replace the entire `download_hls` function:

```rust
async fn download_hls(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&str>,
    dest: &std::path::Path,
    progress_key: Option<String>,
    progress_map: Option<crate::progress::ProgressMap>,
) -> Result<(), anyhow::Error> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};

    let mut headers = String::from(
        "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36\r\n"
    );
    if let Some(r) = referer { headers.push_str(&format!("Referer: {}\r\n", r)); }
    if let Some(c) = cookies { headers.push_str(&format!("Cookie: {}\r\n", c)); }

    let part = dest.with_extension("tmp");
    tracing::info!("HLS download (ffmpeg): {} → {}", url, dest.display());

    let mut child = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-headers", &headers,
            "-i", url,
            "-map", "0:V?",
            "-map", "0:a?",
            "-c", "copy",
            "-f", "mp4",
            "-progress", "pipe:1",
            part.to_str().unwrap_or(""),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr_stream = child.stderr.take().unwrap();

    // Shared total duration in microseconds (parsed from ffmpeg stderr)
    let total_us: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let total_us_stderr = total_us.clone();
    let total_us_stdout = total_us.clone();

    // Parse stderr for "Duration: HH:MM:SS.ss"
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr_stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if total_us_stderr.lock().map(|g| g.is_none()).unwrap_or(false) {
                if let Some(dur_str) = line.split("Duration:").nth(1) {
                    let part = dur_str.trim().split(',').next().unwrap_or("").trim();
                    if let Some(us) = parse_duration_us(part) {
                        if let Ok(mut g) = total_us_stderr.lock() { *g = Some(us); }
                    }
                }
            }
        }
    });

    // Parse stdout (-progress pipe:1) for out_time_us= and update progress map
    let pk = progress_key.clone();
    let pm = progress_map.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(val) = line.strip_prefix("out_time_us=") {
                if let (Ok(out_us), Some(key), Some(ref map)) =
                    (val.trim().parse::<u64>(), pk.as_deref(), pm.as_ref())
                {
                    let total = total_us_stdout.lock().ok().and_then(|g| *g).unwrap_or(0);
                    if total > 0 {
                        crate::progress::set(map, key, out_us as f32 / total as f32);
                    }
                }
            }
        }
    });

    let status = child.wait().await?;

    // Clean up progress entry
    if let (Some(key), Some(ref map)) = (progress_key.as_deref(), progress_map.as_ref()) {
        crate::progress::remove(map, key);
    }

    if !status.success() {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(anyhow::anyhow!("ffmpeg exited with status {}", status));
    }

    if let Err(e) = extract_thumbnail(&part, dest).await {
        tracing::warn!("thumbnail extraction failed for {}: {}", dest.display(), e);
    }
    tokio::fs::rename(&part, dest).await?;
    tracing::info!("HLS download complete: {}", dest.display());
    Ok(())
}
```

Add the duration parser helper (add before or after `sanitize_name`):

```rust
fn parse_duration_us(s: &str) -> Option<u64> {
    // Parses "HH:MM:SS.ss" → microseconds
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 { return None; }
    let h: u64 = parts[0].trim().parse().ok()?;
    let m: u64 = parts[1].trim().parse().ok()?;
    let sec: f64 = parts[2].trim().parse().ok()?;
    Some((h * 3600 + m * 60) * 1_000_000 + (sec * 1_000_000.0) as u64)
}
```

- [ ] **Step 5: Update `submit.rs` to pass progress args to `download`**

In `server/src/handlers/submit.rs`, inside the `tokio::spawn` block for direct downloads, change the `download` call to:

```rust
let submission_id = if reused {
    // Fetch the id for this URL
    match crate::db::get_submission_by_url(&pool, &url).await {
        Ok(Some(s)) => s.id,
        _ => url.clone(),
    }
} else {
    // id was generated just above — clone it before moving into spawn
    // NOTE: capture `id` by cloning before the spawn
    id_for_progress.clone()
};
```

Actually, the cleanest way is to capture `id` before the spawn. Restructure the direct-download block in `submit.rs` as follows (replace from `if is_direct {` through the `return` statement):

```rust
if is_direct {
    let url       = body.url.clone();
    let referer   = body.referer.clone();
    let title     = body.title.clone();
    let cookies   = body.cookies.clone();
    let pool      = state.pool.clone();
    let dl_dir    = state.config.downloads_dir.to_string_lossy().to_string();
    let prog_map  = state.progress.clone();

    // Determine the submission id (either freshly created or the reused one)
    let prog_key: Option<String> = if reused {
        crate::db::get_submission_by_url(&pool, &url).await
            .ok().flatten().map(|s| s.id)
    } else {
        // `id` was bound in the `if !reused` block above — re-fetch from DB
        // since we don't have it in scope here; use a separate variable
        crate::db::get_submission_by_url(&pool, &url).await
            .ok().flatten().map(|s| s.id)
    };

    tokio::spawn(async move {
        // Set to downloading immediately so UI shows active state
        if let Some(ref key) = prog_key {
            crate::progress::set(&prog_map, key, 0.0);
        }
        match crate::direct_download::download(
            &url,
            referer.as_deref(),
            title.as_deref(),
            cookies.as_deref(),
            &dl_dir,
            prog_key,
            Some(prog_map),
        ).await {
            Ok(filename) => {
                let _ = crate::db::mark_imported_by_url(&pool, &url, &filename).await;
            }
            Err(e) => {
                tracing::error!(error = %e, url = %url, "direct download failed");
                let _ = crate::db::mark_pending_as_error_by_url(&pool, &url).await;
            }
        }
    });
    return (StatusCode::OK, Json(SubmitResponse { status: "queued".into() })).into_response();
}
```

> Note: `get_submission_by_url` does a DB round-trip but this runs once at submit time so it's fine.

- [ ] **Step 6: Run tests**

```bash
cd server && cargo test 2>&1 | tail -10
```
Expected: all 25 tests pass (including new `sanitize_removes_null_byte`).

- [ ] **Step 7: Commit**

```bash
git add server/src/direct_download.rs server/src/handlers/submit.rs
git commit -m "feat: parse ffmpeg progress in real-time for HLS direct downloads"
```

---

### Task 4: MeTube Progress

Parse the `percent` field from MeTube's queue response and write it into the ProgressMap keyed by submission ID.

**Files:**
- Modify: `server/src/metube.rs`
- Modify: `server/src/poller.rs`
- Modify: `server/src/main.rs`

**Interfaces:**
- Consumes: `ProgressMap` from Task 2
- Produces: `QueueItem.percent: Option<f64>`, `poller::start(metube_url, pool, progress_map)`

- [ ] **Step 1: Write a failing test for percent parsing**

In `server/src/metube.rs` tests, add:

```rust
#[tokio::test]
async fn parses_percent_from_queue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "queue": [{"url": "https://example.com/v", "title": "Test", "percent": 42.5}],
            "pending": [],
            "done": []
        })))
        .mount(&server)
        .await;

    let state = get_queue_state(&server.uri()).await.unwrap();
    assert_eq!(state.active[0].percent, Some(42.5));
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
cd server && cargo test metube::tests::parses_percent 2>&1 | tail -5
```
Expected: FAIL — `QueueItem` has no `percent` field.

- [ ] **Step 3: Add `percent` to `QueueItem` and parse it**

In `server/src/metube.rs`, update the struct and parsing:

```rust
pub struct QueueItem {
    pub url: String,
    pub title: Option<String>,
    pub percent: Option<f64>,
}
```

In `extract_items`, add percent extraction:

```rust
let percent = item["percent"].as_f64();
Some(QueueItem { url, title, percent })
```

- [ ] **Step 4: Update `poller::start` to accept a ProgressMap and write percent**

Replace all of `server/src/poller.rs`:

```rust
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

                    for item in &state.active {
                        if let Err(e) = crate::db::mark_downloading(&pool, &item.url).await {
                            error!(error = %e, url = %item.url, "db error marking as downloading");
                        }
                        // Write MeTube percent into the progress map keyed by submission id
                        if let Some(pct) = item.percent {
                            match crate::db::get_submission_by_url(&pool, &item.url).await {
                                Ok(Some(sub)) => {
                                    crate::progress::set(&progress, &sub.id, (pct / 100.0) as f32);
                                }
                                Ok(None) => {}
                                Err(e) => error!(error = %e, url = %item.url, "db error fetching sub for progress"),
                            }
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
                        // Clean up progress on error
                        if let Ok(Some(sub)) = crate::db::get_submission_by_url(&pool, &item.url).await {
                            crate::progress::remove(&progress, &sub.id);
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
```

- [ ] **Step 5: Update `main.rs` to pass `progress_map` to `poller::start`**

Find `poller::start(config.metube_url.clone(), pool.clone());` and change it to:

```rust
poller::start(config.metube_url.clone(), pool.clone(), progress_map.clone());
```

- [ ] **Step 6: Run tests**

```bash
cd server && cargo test 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add server/src/metube.rs server/src/poller.rs server/src/main.rs
git commit -m "feat: parse MeTube percent and write to ProgressMap in poller"
```

---

### Task 5: PeerTube Transcoding Poller

After a video is uploaded (`imported` with `peertube_uuid`), poll PeerTube every 30 s and transition the row through `imported → transcoding → complete`.

**Files:**
- Create: `server/src/transcoding_poller.rs`
- Modify: `server/src/main.rs`

**Interfaces:**
- Consumes: `db::mark_transcoding`, `db::mark_complete` from Task 1; `peertube::PeerTubeConfig` (extract to shared type or pass url/host/user/pass directly)
- Produces: `transcoding_poller::start(pool, pt_url, pt_host, pt_user, pt_pass)`

- [ ] **Step 1: Create `server/src/transcoding_poller.rs`**

```rust
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

pub fn start(
    pool: Arc<SqlitePool>,
    pt_url: String,
    pt_host: Option<String>,
    pt_user: String,
    pt_pass: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            // Find all submissions with a peertube_uuid that are not yet complete
            let rows: Vec<(String, String)> = match sqlx::query_as(
                "SELECT id, peertube_uuid FROM submissions WHERE peertube_uuid IS NOT NULL AND status IN ('imported', 'transcoding')"
            )
            .fetch_all(pool.as_ref())
            .await {
                Ok(r) => r,
                Err(e) => { error!(error = %e, "transcoding poller db error"); continue; }
            };

            if rows.is_empty() { continue; }

            let token = match fetch_token(&pt_url, pt_host.as_deref(), &pt_user, &pt_pass).await {
                Ok(t) => t,
                Err(e) => { warn!(error = %e, "transcoding poller: could not get PeerTube token"); continue; }
            };

            for (sub_id, uuid) in rows {
                match fetch_video_state(&pt_url, pt_host.as_deref(), &token, &uuid).await {
                    Ok(state_id) => {
                        if state_id == 1 {
                            // Published — transcoding done
                            if let Err(e) = crate::db::mark_complete(&pool, &uuid).await {
                                error!(error = %e, uuid = %uuid, "transcoding poller: mark_complete error");
                            } else {
                                info!(sub_id = %sub_id, uuid = %uuid, "video transcoding complete");
                            }
                        } else {
                            // Still transcoding
                            if let Err(e) = crate::db::mark_transcoding(&pool, &uuid).await {
                                error!(error = %e, uuid = %uuid, "transcoding poller: mark_transcoding error");
                            }
                        }
                    }
                    Err(e) => warn!(error = %e, uuid = %uuid, "transcoding poller: could not fetch video state"),
                }
            }
        }
    })
}

async fn fetch_token(url: &str, host: Option<&str>, username: &str, password: &str) -> anyhow::Result<String> {
    use serde::Deserialize;
    #[derive(Deserialize)] struct OAuthClient { client_id: String, client_secret: String }
    #[derive(Deserialize)] struct TokenResp { access_token: String }

    let h = host.map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));
    let client = reqwest::Client::new();

    let body = client.get(format!("{}/api/v1/oauth-clients/local", url))
        .header("Host", &h).send().await?.text().await?;
    let oauth: OAuthClient = serde_json::from_str(&body)?;

    let body = client.post(format!("{}/api/v1/users/token", url))
        .header("Host", &h)
        .form(&[
            ("client_id", oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type", "password"),
            ("response_type", "code"),
            ("username", username),
            ("password", password),
        ])
        .send().await?.text().await?;
    let token: TokenResp = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("token parse error ({e}): {body}"))?;
    Ok(token.access_token)
}

async fn fetch_video_state(url: &str, host: Option<&str>, token: &str, uuid: &str) -> anyhow::Result<u64> {
    use serde::Deserialize;
    #[derive(Deserialize)] struct State { id: u64 }
    #[derive(Deserialize)] struct Video { state: State }

    let h = host.map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));
    let client = reqwest::Client::new();

    let resp = client.get(format!("{}/api/v1/videos/{}", url, uuid))
        .header("Host", &h)
        .bearer_auth(token)
        .send().await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("PeerTube returned {}", resp.status()));
    }
    let body = resp.text().await?;
    let video: Video = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("video parse error ({e}): {body}"))?;
    Ok(video.state.id)
}

fn derive_host(url: &str) -> String {
    if let Ok(parsed) = url.parse::<reqwest::Url>() {
        if let Some(host) = parsed.host_str() {
            return match parsed.port() {
                Some(p) => format!("{}:{}", host, p),
                None => host.to_string(),
            };
        }
    }
    url.to_string()
}
```

- [ ] **Step 2: Register module and start poller in `main.rs`**

Add `mod transcoding_poller;` to the module list.

After the `watcher::start(...)` call, add:

```rust
if let (Some(pt_url), Some(pt_user), Some(pt_pass)) = (
    &config.peertube_url,
    &config.peertube_username,
    &config.peertube_password,
) {
    transcoding_poller::start(
        pool.clone(),
        pt_url.clone(),
        config.peertube_host.clone(),
        pt_user.clone(),
        pt_pass.clone(),
    );
}
```

- [ ] **Step 3: Build**

```bash
cd server && cargo build 2>&1 | grep -E "^error" | head -10
```
Expected: no errors.

- [ ] **Step 4: Run all tests**

```bash
cd server && cargo test 2>&1 | tail -10
```
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add server/src/transcoding_poller.rs server/src/main.rs
git commit -m "feat: add PeerTube transcoding poller (imported → transcoding → complete)"
```

---

### Task 6: UI Progress Bars + Wiring

Add a thin progress bar to each card's thumbnail area, animate it for active downloads, and connect it to the `progress` field from the API.

**Files:**
- Modify: `server/static/style.css`
- Modify: `server/templates/dashboard.html`

**Interfaces:**
- Consumes: `SubmissionRow.progress: Option<f32>` from Task 2 (serialized as 0.0–1.0 float or null)

- [ ] **Step 1: Add progress bar CSS**

In `server/static/style.css`, add after the shimmer keyframes block (after line `a.card.processing .card-thumb::after { ... }`):

```css
/* Progress bar — sits at the bottom of the thumbnail */
.card-progress {
  position: absolute;
  bottom: 0;
  left: 0;
  height: 3px;
  background: var(--accent);
  width: 0%;
  transition: width 0.4s ease;
  z-index: 3;
  border-radius: 0 2px 2px 0;
}
.card-progress[hidden] { display: none; }
```

- [ ] **Step 2: Add the progress bar element to `buildCard`**

In `dashboard.html`, inside `buildCard`, replace the `card.innerHTML = ...` string. Find the `.card-thumb` div in the innerHTML and add the progress bar inside it:

```js
card.innerHTML =
  '<input type="checkbox" class="card-checkbox" aria-label="Select">' +
  '<div class="card-thumb">' +
    '<div class="card-progress" hidden></div>' +
  '</div>' +
  '<div class="card-body">' +
    '<div class="card-site"></div>' +
    '<div class="card-title">' + (s.title ? escHtml(s.title) : '') + '</div>' +
    '<div class="card-url">' + escHtml(s.url) + '</div>' +
    '<div class="card-footer">' +
      '<span class="card-status">' +
        '<span class="status-dot status-' + s.status + '"></span>' + s.status +
      '</span>' +
      '<span class="card-time">' + relTime(s.submitted_at) + '</span>' +
    '</div>' +
  '</div>' +
  '<button class="card-delete-btn" title="Delete" aria-label="Delete">🗑</button>';
```

- [ ] **Step 3: Update `updateCardStatus` to set progress bar**

In `updateCardStatus`, add after the `card.classList.toggle('processing', proc)` line:

```js
// Update progress bar
const bar = card.querySelector('.card-progress');
if (bar) {
  const pct = (s.progress != null) ? s.progress : null;
  if (pct != null && proc) {
    bar.hidden = false;
    bar.style.width = Math.round(pct * 100) + '%';
    bar.title = Math.round(pct * 100) + '%';
  } else {
    bar.hidden = true;
    bar.style.width = '0%';
  }
}

// Update status label to include % when downloading
const statusLabel = card.querySelector('.card-status');
if (statusLabel) {
  const pct = (s.progress != null && (s.status === 'downloading' || s.status === 'pending'))
    ? ' ' + Math.round(s.progress * 100) + '%'
    : '';
  statusLabel.innerHTML =
    '<span class="status-dot status-' + s.status + '"></span>' +
    s.status + escHtml(pct);
}
```

- [ ] **Step 4: Update `openDetail` to show progress in the status line**

In `openDetail`, replace the `status.innerHTML = ...` line:

```js
const pctLabel = (s.progress != null && isProcessing(s.status))
  ? ' — ' + Math.round(s.progress * 100) + '%'
  : '';
status.innerHTML =
  '<span class="status-dot status-' + s.status + '"></span>' +
  '<span>' + s.status + escHtml(pctLabel) + '</span>';
```

- [ ] **Step 5: Shorten poll interval when any card is actively downloading**

The existing `schedulePoll` polls every 5s when `isProcessing` is true. Since `isProcessing` now includes `transcoding`, reduce the transcoding poll to 15s (faster than the 30s DB poller) to keep UI snappy. Replace `schedulePoll`:

```js
function schedulePoll(submissions) {
  clearTimeout(pollTimer);
  const hasDownloading = submissions.some(s => s.status === 'pending' || s.status === 'downloading');
  const hasTranscoding = submissions.some(s => s.status === 'transcoding');
  const delay = hasDownloading ? 3000 : hasTranscoding ? 15000 : 30000;
  pollTimer = setTimeout(async () => {
    const data = await loadPage(currentPage, currentStatus, currentSearch);
    if (data) applyData(data, true);
  }, delay);
}
```

Update the `applyData` call to `schedulePoll` to pass the submissions array:

```js
schedulePoll(data.submissions);
```

- [ ] **Step 6: Build the server to catch any Rust compilation issues from earlier tasks**

```bash
cd server && cargo build --release 2>&1 | grep -E "^error" | head -10
```
Expected: no errors.

- [ ] **Step 7: Run all tests**

```bash
cd server && cargo test 2>&1 | tail -15
```
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add server/static/style.css server/templates/dashboard.html
git commit -m "feat: add live progress bars and granular state display to dashboard"
```

---

## Self-Review Checklist

**Spec coverage:**
- [x] More granular states (`downloading`, `transcoding`, `complete`) — Tasks 1 + 5
- [x] Progress bar for ffmpeg HLS path — Task 3
- [x] Progress bar for MeTube downloads — Task 4
- [x] PeerTube transcoding poller — Task 5
- [x] UI shows % on cards and in detail modal — Task 6
- [x] Filter tabs include new states — Task 1
- [x] Poll interval tuned per state — Task 6

**Type consistency:**
- `ProgressMap` = `Arc<Mutex<HashMap<String, f32>>>` used in Tasks 2, 3, 4
- `progress::set(map, id, pct)` / `progress::remove(map, id)` / `progress::get(map, id)` used consistently
- `mark_transcoding(pool, uuid)` / `mark_complete(pool, uuid)` used in Tasks 1 and 5
- `poller::start(url, pool, progress_map)` — 3 args, used in Tasks 4 and updated in main.rs

**Potential gotcha:** `get_submission_by_url` in `submit.rs` (Task 3) does a DB round-trip right after creating the row — should be fine since we just inserted it and SQLite serializes writes.

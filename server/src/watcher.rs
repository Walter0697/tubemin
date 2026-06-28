use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{error, info};

pub struct PeerTubeConfig {
    pub url: String,
    pub host: Option<String>,
    pub username: String,
    pub password: String,
}

pub fn start(
    downloads_dir: PathBuf,
    import_dir: PathBuf,
    pool: Arc<SqlitePool>,
    peertube: Option<PeerTubeConfig>,
) -> tokio::task::JoinHandle<()> {
    let peertube = Arc::new(peertube);
    tokio::spawn(async move {
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // Channel for inotify events → async task
        let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();

        // Spawn blocking thread running the notify watcher
        let watch_dir = downloads_dir.clone();
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            use notify::{Watcher, RecursiveMode, recommended_watcher, Event, EventKind};
            use notify::event::CreateKind;

            let tx3 = tx2.clone();
            let mut watcher = match recommended_watcher(move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if matches!(event.kind, EventKind::Create(CreateKind::File) | EventKind::Modify(_)) {
                        for path in event.paths {
                            let _ = tx3.send(path);
                        }
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    error!("Failed to create file watcher: {e}. Falling back to poll only.");
                    return;
                }
            };

            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                error!("Failed to watch {}: {e}. Falling back to poll only.", watch_dir.display());
                return;
            }

            info!("Watching {} for new files", watch_dir.display());
            // Keep thread alive (watcher drops when thread exits)
            loop { std::thread::sleep(std::time::Duration::from_secs(3600)); }
        });

        // Fallback: also scan every 30s to catch anything inotify missed
        let scan_tx = tx.clone();
        let scan_dir = downloads_dir.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                if let Ok(entries) = std::fs::read_dir(&scan_dir) {
                    for entry in entries.flatten() {
                        let _ = scan_tx.send(entry.path());
                    }
                }
            }
        });

        // Also do an initial scan on startup
        if let Ok(entries) = std::fs::read_dir(&downloads_dir) {
            for entry in entries.flatten() {
                let _ = tx.send(entry.path());
            }
        }

        while let Some(path) = rx.recv().await {
            if !path.is_file() || is_temp_file(&path) || is_image_file(&path) || seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());
            let path_key = path.clone();

            // Read thumbnail bytes before the video is moved out of /downloads
            let thumbnail = crate::video_meta::find_thumbnail_path(&path).and_then(|tp| {
                let mime = if tp.extension().map_or(false, |e| e == "webp") { "image/webp" } else { "image/jpeg" };
                std::fs::read(&tp).ok().map(|bytes| {
                    let _ = std::fs::remove_file(&tp);
                    (bytes, mime.to_string())
                })
            });

            let meta = crate::video_meta::load_for(&path);
            let dest = handle_new_file(path, &import_dir, &pool).await;
            // Remove from seen on success so a future file with the same name
            // (e.g. a re-download after the first was moved out) gets processed.
            if dest.is_some() {
                seen.remove(&path_key);
            }
            if let (Some(dest), Some(pt)) = (dest, peertube.as_ref().as_ref()) {
                let thumb_arg = thumbnail.as_ref().map(|(b, m)| (b.clone(), m.as_str()));
                match crate::peertube::upload(&pt.url, pt.host.as_deref(), &pt.username, &pt.password, &dest, &meta, thumb_arg).await {
                    Ok((preview_path, peertube_uuid)) => {
                        info!("Uploaded {} to PeerTube", dest.display());
                        let filename = dest.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if let Err(e) = crate::db::set_peertube_thumb(&pool, filename, &preview_path, &peertube_uuid).await {
                            error!("db error storing peertube thumb for {}: {}", filename, e);
                        }
                    }
                    Err(e) => error!("PeerTube upload failed for {}: {}", dest.display(), e),
                }
            }
        }
    })
}

pub(crate) fn is_image_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jpg") | Some("jpeg") | Some("webp") | Some("png")
    )
}

pub(crate) fn is_temp_file(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "part" | "ytdl" | "tmp" | "json") {
        return true;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // MeTube in-progress: video.temp.ext
    if stem.ends_with(".temp") {
        return true;
    }
    // yt-dlp adaptive stream fragments before ffmpeg merge: video.f251.webm, video.f399.mp4
    if let Some(stem_ext) = std::path::Path::new(stem).extension().and_then(|e| e.to_str()) {
        if stem_ext.starts_with('f') && stem_ext[1..].chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

pub(crate) async fn handle_new_file(
    path: PathBuf,
    import_dir: &PathBuf,
    pool: &SqlitePool,
) -> Option<PathBuf> {
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return None,
    };
    let dest = import_dir.join(&filename);
    let move_result = std::fs::copy(&path, &dest)
        .and_then(|_| std::fs::remove_file(&path));
    match move_result {
        Ok(_) => {
            info!("Moved {} to import dir", filename);
            let _ = std::fs::remove_file(path.with_extension("info.json"));
            // Skip mark_imported if a direct download already claimed this filename
            let already: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM submissions WHERE filename = ? AND status = 'imported'"
            ).bind(&filename).fetch_optional(pool).await.unwrap_or(None);
            if already.is_none() {
                let _ = crate::db::mark_imported(pool, &filename).await;
            }
            Some(dest)
        }
        Err(e) => {
            error!("Failed to move {}: {}", filename, e);
            let _ = crate::db::mark_error(pool, &filename).await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_detection() {
        assert!(is_temp_file(std::path::Path::new("/downloads/video.part")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.ytdl")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.temp.webm")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.temp.mp4")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.f251.webm")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.f399.mp4")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.info.json")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mp4")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mkv")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.webm")));
    }

    #[tokio::test]
    async fn file_moved_to_import_dir() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let pool = Arc::new(crate::db::init("sqlite::memory:").await.unwrap());

        let test_file = src_dir.path().join("video.mp4");
        std::fs::write(&test_file, b"fake video").unwrap();

        let result = handle_new_file(
            test_file.clone(),
            &dst_dir.path().to_path_buf(),
            &pool,
        ).await;

        assert!(result.is_some());
        assert!(!test_file.exists(), "source file should be moved");
        assert!(dst_dir.path().join("video.mp4").exists(), "dest file should exist");
    }
}

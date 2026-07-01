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
                if let Ok(mut dir) = tokio::fs::read_dir(&scan_dir).await {
                    while let Ok(Some(entry)) = dir.next_entry().await {
                        let _ = scan_tx.send(entry.path());
                    }
                }
            }
        });

        // Also do an initial scan on startup
        if let Ok(mut dir) = tokio::fs::read_dir(&downloads_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let _ = tx.send(entry.path());
            }
        }

        while let Some(path) = rx.recv().await {
            if !path.is_file() || is_temp_file(&path) || is_image_file(&path) || is_subtitle_file(&path) || seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());

            // Read thumbnail bytes before the video is moved out of /downloads
            let thumbnail = crate::video_meta::find_thumbnail_path(&path).and_then(|tp| {
                let mime = if tp.extension().map_or(false, |e| e == "webp") { "image/webp" } else { "image/jpeg" };
                std::fs::read(&tp).ok().map(|bytes| {
                    let _ = std::fs::remove_file(&tp);
                    (bytes, mime.to_string())
                })
            });

            // Collect subtitle sidecars before the video is moved out of /downloads.
            // yt-dlp writes them alongside the video as {stem}.{lang}.vtt.
            let subtitles = find_subtitle_sidecars(&path);

            let meta = crate::video_meta::load_for(&path);
            let dest = handle_new_file(&path, &import_dir, &pool).await;
            // Remove from seen on success so a future file with the same name
            // (e.g. a re-download after the first was moved out) gets processed.
            if dest.is_some() {
                seen.remove(&path);
            }
            if let (Some(dest), Some(pt)) = (dest, peertube.as_ref().as_ref()) {
                let thumb_arg = thumbnail.as_ref().map(|(b, m)| (b.clone(), m.as_str()));
                // Direct downloads have no .info.json so meta.title is None.
                // Pull the stored title from DB so PeerTube and the dashboard show the same thing.
                let mut meta = meta;
                if meta.title.is_none() {
                    let fname = dest.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if let Ok(title) = crate::db::get_title_by_filename(pool.as_ref(), fname).await {
                        meta.title = title;
                    }
                }
                match crate::peertube::upload(&pt.url, pt.host.as_deref(), &pt.username, &pt.password, &dest, &meta, thumb_arg).await {
                    Ok((preview_path, peertube_uuid)) => {
                        info!("Uploaded {} to PeerTube", dest.display());
                        let filename = dest.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if let Err(e) = crate::db::set_peertube_thumb(&pool, filename, &preview_path, &peertube_uuid).await {
                            error!("db error storing peertube thumb for {}: {}", filename, e);
                        }
                        // Upload subtitle captions to PeerTube
                        if !subtitles.is_empty() {
                            let caption_data: Vec<(String, Vec<u8>)> = subtitles.iter()
                                .filter_map(|(lang, p)| std::fs::read(p).ok().map(|b| (lang.clone(), b)))
                                .collect();
                            if let Err(e) = crate::peertube::upload_captions(
                                &pt.url, pt.host.as_deref(), &pt.username, &pt.password,
                                &peertube_uuid, &caption_data,
                            ).await {
                                error!("Caption upload failed for {}: {}", peertube_uuid, e);
                            }
                        }
                    }
                    Err(e) => error!("PeerTube upload failed for {}: {}", dest.display(), e),
                }
            }
            // Clean up sidecar subtitle files whether or not PeerTube is configured
            for (_, sub_path) in &subtitles {
                let _ = std::fs::remove_file(sub_path);
            }
        }
    })
}

pub(crate) fn is_subtitle_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("vtt") | Some("srt") | Some("ass") | Some("ssa") | Some("sub")
    )
}

// Returns sidecar subtitle files next to `video_path` as (lang_code, path) pairs.
// yt-dlp names them `{stem}.{lang}.vtt`, e.g. `My Video.en.vtt`.
fn find_subtitle_sidecars(video_path: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return vec![],
    };
    let dir = match video_path.parent() {
        Some(d) => d,
        None => return vec![],
    };
    let prefix = format!("{}.", stem);
    let mut results = vec![];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_subtitle_file(&path) { continue; }
            let fname = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if !fname.starts_with(&prefix) { continue; }
            // "My Video.en.vtt" → rest after prefix = "en.vtt"
            let rest = &fname[prefix.len()..];
            if let Some(dot) = rest.rfind('.') {
                let lang = &rest[..dot];
                if !lang.is_empty() {
                    results.push((lang.to_string(), path));
                }
            }
        }
    }
    results
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
    path: &std::path::Path,
    import_dir: &PathBuf,
    pool: &SqlitePool,
) -> Option<PathBuf> {
    let filename = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return None,
    };
    let dest = import_dir.join(&filename);
    let move_result = match tokio::fs::copy(path, &dest).await {
        Ok(_) => tokio::fs::remove_file(path).await,
        Err(e) => Err(e),
    };
    match move_result {
        Ok(_) => {
            info!("Moved {} to import dir", filename);
            let _ = tokio::fs::remove_file(path.with_extension("info.json")).await;
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
            &test_file,
            &dst_dir.path().to_path_buf(),
            &pool,
        ).await;

        assert!(result.is_some());
        assert!(!test_file.exists(), "source file should be moved");
        assert!(dst_dir.path().join("video.mp4").exists(), "dest file should exist");
    }
}

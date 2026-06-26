use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

const POLL_INTERVAL_SECS: u64 = 5;

pub struct PeerTubeConfig {
    pub url: String,
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
        let mut ticker = interval(Duration::from_secs(POLL_INTERVAL_SECS));

        loop {
            ticker.tick().await;

            let entries = match std::fs::read_dir(&downloads_dir) {
                Ok(e) => e,
                Err(e) => { error!("Failed to read downloads dir: {}", e); continue; }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || is_temp_file(&path) || seen.contains(&path) {
                    continue;
                }
                seen.insert(path.clone());
                let dest = handle_new_file(path, &import_dir, &pool).await;
                if let (Some(dest), Some(pt)) = (dest, peertube.as_ref().as_ref()) {
                    match crate::peertube::upload(&pt.url, &pt.username, &pt.password, &dest).await {
                        Ok(_) => info!("Uploaded {} to PeerTube", dest.display()),
                        Err(e) => error!("PeerTube upload failed for {}: {}", dest.display(), e),
                    }
                }
            }
        }
    })
}

pub(crate) fn is_temp_file(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "part" | "ytdl" | "tmp") {
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
            let _ = crate::db::mark_imported(pool, &filename).await;
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

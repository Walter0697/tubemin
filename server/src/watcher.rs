use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Config as NotifyConfig};
use notify::event::{EventKind};
use tracing::{error, info};

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

        // Process any files already present before the watcher started
        let rt = tokio::runtime::Handle::current();
        if let Ok(entries) = std::fs::read_dir(&downloads_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && !is_temp_file(&path) {
                    let import_dir = import_dir.clone();
                    let pool = pool.clone();
                    rt.spawn(async move {
                        handle_new_file(path, &import_dir, &pool).await;
                    });
                }
            }
        }

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

pub(crate) fn is_temp_file(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "part" | "ytdl" | "tmp") {
        return true;
    }
    // MeTube names in-progress files as video.temp.webm — stem ends with ".temp"
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.ends_with(".temp"))
        .unwrap_or(false)
}

pub(crate) async fn handle_new_file(
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
    let move_result = std::fs::copy(&path, &dest)
        .and_then(|_| std::fs::remove_file(&path));
    match move_result {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_detection() {
        assert!(is_temp_file(std::path::Path::new("/downloads/video.part")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.ytdl")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.temp.webm")));
        assert!(is_temp_file(std::path::Path::new("/downloads/video.temp.mp4")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mp4")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.mkv")));
        assert!(!is_temp_file(std::path::Path::new("/downloads/video.webm")));
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

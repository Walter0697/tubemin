use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
pub struct VideoMeta {
    pub title: Option<String>,
    pub description: Option<String>,
    pub uploader: Option<String>,
    pub upload_date: Option<String>, // yt-dlp format: YYYYMMDD
}

pub fn find_thumbnail_path(video_path: &Path) -> Option<PathBuf> {
    for ext in &["jpg", "jpeg", "webp", "png"] {
        let p = video_path.with_extension(ext);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn load_for(video_path: &Path) -> VideoMeta {
    let info_path = video_path.with_extension("info.json");
    let data = match std::fs::read_to_string(&info_path) {
        Ok(d) => d,
        Err(_) => return VideoMeta::default(),
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn format_description(meta: &VideoMeta) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(uploader) = &meta.uploader {
        parts.push(format!("Originally uploaded by: {}", uploader));
    }
    if let Some(desc) = &meta.description {
        if !desc.is_empty() {
            parts.push(desc.clone());
        }
    }
    parts.join("\n\n")
}

// yt-dlp upload_date is YYYYMMDD; PeerTube expects ISO 8601
pub fn upload_date_to_iso(date: &str) -> Option<String> {
    if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("{}-{}-{}T00:00:00.000Z", &date[..4], &date[4..6], &date[6..8]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_date_converts_correctly() {
        assert_eq!(upload_date_to_iso("20240315"), Some("2024-03-15T00:00:00.000Z".into()));
        assert_eq!(upload_date_to_iso("bad"), None);
        assert_eq!(upload_date_to_iso(""), None);
    }

    #[test]
    fn format_description_with_all_fields() {
        let meta = VideoMeta {
            uploader: Some("MyChan".into()),
            description: Some("Cool video".into()),
            ..Default::default()
        };
        let desc = format_description(&meta);
        assert!(desc.contains("Originally uploaded by: MyChan"));
        assert!(desc.contains("Cool video"));
    }

    #[test]
    fn format_description_missing_uploader() {
        let meta = VideoMeta {
            description: Some("Just a description".into()),
            ..Default::default()
        };
        let desc = format_description(&meta);
        assert_eq!(desc, "Just a description");
    }

    #[test]
    fn load_for_missing_file_returns_default() {
        let meta = load_for(Path::new("/nonexistent/video.mp4"));
        assert!(meta.title.is_none());
        assert!(meta.description.is_none());
    }

    #[test]
    fn load_for_parses_json() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("video.mp4");
        let info = dir.path().join("video.info.json");
        std::fs::write(&info, r#"{"title":"My Vid","description":"Desc","uploader":"Chan","upload_date":"20230101"}"#).unwrap();
        let meta = load_for(&video);
        assert_eq!(meta.title.as_deref(), Some("My Vid"));
        assert_eq!(meta.uploader.as_deref(), Some("Chan"));
        assert_eq!(meta.upload_date.as_deref(), Some("20230101"));
    }
}

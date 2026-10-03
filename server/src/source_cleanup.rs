use std::path::{Path, PathBuf};

pub fn is_safe_filename(filename: &str) -> bool {
    let path = Path::new(filename);
    !filename.is_empty() && path.file_name().and_then(|name| name.to_str()) == Some(filename)
}

pub fn source_artifacts(root: &Path, filename: &str) -> Vec<PathBuf> {
    let video = root.join(filename);
    let mut paths = vec![video.clone(), video.with_extension("info.json")];
    let Some(stem) = video.file_stem().and_then(|stem| stem.to_str()) else {
        return paths;
    };
    for extension in ["jpg", "jpeg", "png", "webp"] {
        paths.push(root.join(format!("{stem}.{extension}")));
    }
    let prefix = format!("{stem}.");
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with(&prefix)
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("vtt" | "srt" | "ass" | "ssa" | "sub")
                )
            {
                paths.push(path);
            }
        }
    }
    paths
}

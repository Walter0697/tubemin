use std::path::Path;
use reqwest::Client;
use tokio::io::AsyncWriteExt;
use tracing::info;

pub async fn download(
    url: &str,
    referer: Option<&str>,
    title: Option<&str>,
    cookies: Option<&str>,
    downloads_dir: &str,
) -> Result<(), anyhow::Error> {
    let url_path = url.split('?').next().unwrap_or(url);
    let is_hls = url_path.ends_with(".m3u8") || url_path.ends_with(".mpd");

    let base = title
        .map(sanitize_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            url_path
                .rsplit('/')
                .next()
                .and_then(|seg| seg.split('.').next())
                .unwrap_or("video")
                .to_string()
        });

    let dest = unique_dest(downloads_dir, &base, ".mp4");

    if is_hls {
        download_hls(url, referer, cookies, &dest).await
    } else {
        download_direct(url, referer, cookies, &dest).await
    }
}

// ── HLS via ffmpeg ─────────────────────────────────────────────────────────

async fn download_hls(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&str>,
    dest: &Path,
) -> Result<(), anyhow::Error> {
    // Build the headers string ffmpeg expects: "Key: Value\r\nKey: Value\r\n"
    let mut headers = String::from(
        "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36\r\n"
    );
    if let Some(r) = referer {
        headers.push_str(&format!("Referer: {}\r\n", r));
    }
    if let Some(c) = cookies {
        headers.push_str(&format!("Cookie: {}\r\n", c));
    }

    let part = dest.with_extension("tmp");
    info!("HLS download (ffmpeg): {} → {}", url, dest.display());

    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-headers", &headers,
            "-i", url,
            "-map", "0:V?",
            "-map", "0:a?",
            "-c", "copy",
            "-f", "mp4",
            part.to_str().unwrap_or(""),
        ])
        .status()
        .await?;

    if !status.success() {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(anyhow::anyhow!("ffmpeg exited with status {}", status));
    }

    // Extract thumbnail before rename so the watcher finds it alongside the .mp4
    if let Err(e) = extract_thumbnail(&part, dest).await {
        tracing::warn!("thumbnail extraction failed for {}: {}", dest.display(), e);
    }
    tokio::fs::rename(&part, dest).await?;
    info!("HLS download complete: {}", dest.display());
    Ok(())
}

// ── Direct MP4/file download via reqwest ───────────────────────────────────

async fn download_direct(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&str>,
    dest: &Path,
) -> Result<(), anyhow::Error> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    let mut builder = client.get(url);
    if let Some(r) = referer { builder = builder.header("Referer", r); }
    if let Some(c) = cookies { builder = builder.header("Cookie", c); }

    let resp = builder.send().await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("CDN returned HTTP {}", resp.status()));
    }

    let part = dest.with_extension("tmp");
    info!("Direct download: {} → {}", url, dest.display());

    let mut file = tokio::fs::File::create(&part).await?;
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    // Extract thumbnail before rename so the watcher finds it alongside the .mp4
    if let Err(e) = extract_thumbnail(&part, dest).await {
        tracing::warn!("thumbnail extraction failed for {}: {}", dest.display(), e);
    }
    tokio::fs::rename(&part, dest).await?;
    info!("Direct download complete: {}", dest.display());
    Ok(())
}

// ── Thumbnail extraction ───────────────────────────────────────────────────

// Reads from `src` (the .tmp file), writes thumbnail named after `dest` (the final .mp4 path).
// Called before the rename so the .jpg exists when the watcher notices the .mp4.
async fn extract_thumbnail(src: &Path, dest: &Path) -> Result<(), anyhow::Error> {
    let thumb_path = dest.with_extension("jpg");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-ss", "5",
            "-i", src.to_str().unwrap_or(""),
            "-vframes", "1",
            "-q:v", "2",
            "-update", "1",   // write a single file, not an image sequence
            thumb_path.to_str().unwrap_or(""),
        ])
        .status()
        .await?;

    if !status.success() {
        return Err(anyhow::anyhow!("ffmpeg exited with status {}", status));
    }
    if !thumb_path.exists() {
        return Err(anyhow::anyhow!("ffmpeg exited successfully but wrote no thumbnail"));
    }
    info!("Thumbnail extracted: {}", thumb_path.display());
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Returns a path that doesn't already exist by appending (2), (3), etc.
fn unique_dest(dir: &str, base: &str, ext: &str) -> std::path::PathBuf {
    let first = Path::new(dir).join(format!("{}{}", base, ext));
    if !first.exists() {
        return first;
    }
    let mut n = 2u32;
    loop {
        let candidate = Path::new(dir).join(format!("{} ({}){}", base, n, ext));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_unicode() {
        assert_eq!(sanitize_name("My Show S01E05 [1080p]"), "My Show S01E05 [1080p]");
        assert_eq!(sanitize_name("My Video (2024)"), "My Video (2024)");
        assert_eq!(sanitize_name("  hello  "), "hello");
        assert_eq!(sanitize_name("file/with\\bad:chars"), "file_with_bad_chars");
        assert_eq!(sanitize_name("한국 드라마 EP01"), "한국 드라마 EP01");
    }
}

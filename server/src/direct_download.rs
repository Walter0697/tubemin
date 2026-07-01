use std::path::Path;
use reqwest::Client;
use tokio::io::AsyncWriteExt;
use tracing::info;

static HTTP_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Download a direct media URL. Returns the filename (not full path) of the completed file.
pub async fn download(
    url: &str,
    referer: Option<&str>,
    title: Option<&str>,
    cookies: Option<&str>,
    downloads_dir: &str,
    progress_key: Option<String>,
    progress_map: Option<crate::progress::ProgressMap>,
) -> Result<String, anyhow::Error> {
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
        download_hls(url, referer, cookies, &dest, progress_key, progress_map).await?;
        // Best-effort: extract subtitle tracks from the master playlist as sidecar .vtt files.
        // The watcher will pick these up and upload them to PeerTube's captions API.
        extract_hls_subtitles(url, referer, cookies, &dest).await;
    } else {
        download_direct(url, referer, cookies, &dest).await?;
    }

    let filename = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video.mp4")
        .to_string();
    Ok(filename)
}

// ── HLS via ffmpeg ─────────────────────────────────────────────────────────

async fn download_hls(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&str>,
    dest: &Path,
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
    info!("HLS download (ffmpeg): {} → {}", url, dest.display());

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

    let total_us: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let total_us_stderr = total_us.clone();
    let total_us_stdout = total_us.clone();

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
    info!("HLS download complete: {}", dest.display());
    Ok(())
}

// ── HLS subtitle extraction ────────────────────────────────────────────────
// Parses #EXT-X-MEDIA:TYPE=SUBTITLES lines from the master playlist and runs
// ffmpeg on each subtitle playlist to produce sidecar .vtt files next to dest.
// Entirely best-effort — any error is logged as a warning.

async fn extract_hls_subtitles(master_url: &str, referer: Option<&str>, cookies: Option<&str>, dest: &Path) {
    let tracks = match fetch_subtitle_tracks(master_url, referer, cookies).await {
        Ok(t) => t,
        Err(e) => { tracing::warn!("subtitle track discovery failed: {}", e); return; }
    };
    if tracks.is_empty() { return; }

    let mut headers = String::from(
        "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36\r\n"
    );
    if let Some(r) = referer { headers.push_str(&format!("Referer: {}\r\n", r)); }
    if let Some(c) = cookies { headers.push_str(&format!("Cookie: {}\r\n", c)); }

    let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let dir = dest.parent().unwrap_or(Path::new("."));

    for (lang, sub_url) in tracks {
        let out = dir.join(format!("{}.{}.vtt", stem, lang));
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-y", "-headers", &headers, "-i", &sub_url, "-f", "webvtt", out.to_str().unwrap_or("")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        match status {
            Ok(s) if s.success() => info!("Extracted {} subtitle → {}", lang, out.display()),
            Ok(s) => tracing::warn!("ffmpeg subtitle extract failed for lang {}: status {}", lang, s),
            Err(e) => tracing::warn!("ffmpeg subtitle extract error for lang {}: {}", lang, e),
        }
    }
}

// Returns (language_code, absolute_subtitle_playlist_url) pairs from a master playlist.
async fn fetch_subtitle_tracks(master_url: &str, referer: Option<&str>, cookies: Option<&str>) -> anyhow::Result<Vec<(String, String)>> {
    let mut req = client().get(master_url);
    if let Some(r) = referer { req = req.header("Referer", r); }
    if let Some(c) = cookies { req = req.header("Cookie", c); }
    let text = req.send().await?.text().await?;

    let mut tracks = vec![];
    for line in text.lines() {
        let line = line.trim();
        // #EXT-X-MEDIA:TYPE=SUBTITLES,...,LANGUAGE="en",...,URI="sub/en.m3u8"
        if !line.starts_with("#EXT-X-MEDIA") { continue; }
        if !line.contains("TYPE=SUBTITLES") { continue; }

        let lang = extract_attr(line, "LANGUAGE").unwrap_or_else(|| "und".to_string());
        let uri = match extract_attr(line, "URI") {
            Some(u) => u,
            None => continue,
        };

        // Resolve relative URIs against the master playlist URL
        let abs_url = if uri.starts_with("http://") || uri.starts_with("https://") {
            uri
        } else {
            match reqwest::Url::parse(master_url).ok().and_then(|base| base.join(&uri).ok()) {
                Some(u) => u.to_string(),
                None => continue,
            }
        };
        tracks.push((lang, abs_url));
    }
    Ok(tracks)
}

fn extract_attr(line: &str, key: &str) -> Option<String> {
    let search = format!("{}=", key);
    let start = line.find(&search)? + search.len();
    let rest = &line[start..];
    if rest.starts_with('"') {
        let end = rest[1..].find('"')? + 1;
        Some(rest[1..end].to_string())
    } else {
        let end = rest.find([',', '\r', '\n']).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

// ── Direct MP4/file download via reqwest ───────────────────────────────────

async fn download_direct(
    url: &str,
    referer: Option<&str>,
    cookies: Option<&str>,
    dest: &Path,
) -> Result<(), anyhow::Error> {
    let mut builder = client().get(url);
    if let Some(r) = referer { builder = builder.header("Referer", r); }
    if let Some(c) = cookies { builder = builder.header("Cookie", c); }

    let mut resp = builder.send().await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("CDN returned HTTP {}", resp.status()));
    }

    let part = dest.with_extension("tmp");
    info!("Direct download: {} → {}", url, dest.display());

    let mut file = tokio::fs::File::create(&part).await?;
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

fn parse_duration_us(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 { return None; }
    let h: u64 = parts[0].trim().parse().ok()?;
    let m: u64 = parts[1].trim().parse().ok()?;
    let sec: f64 = parts[2].trim().parse().ok()?;
    Some((h * 3600 + m * 60) * 1_000_000 + (sec * 1_000_000.0) as u64)
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

    #[test]
    fn sanitize_removes_null_byte() {
        assert_eq!(sanitize_name("hello\0world"), "hello_world");
    }
}

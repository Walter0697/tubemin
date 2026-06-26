use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use tokio_util::io::ReaderStream;

#[derive(Deserialize)]
struct OAuthClient {
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct VideoChannel {
    id: i64,
}

#[derive(Deserialize)]
struct ChannelList {
    data: Vec<VideoChannel>,
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

pub async fn upload(url: &str, username: &str, password: &str, file_path: &Path) -> Result<()> {
    let client = Client::new();

    // 1. Get OAuth client credentials
    let oauth: OAuthClient = client
        .get(format!("{}/api/v1/oauth-clients/local", url))
        .send().await?
        .json().await?;

    // 2. Exchange credentials for access token
    let token: TokenResponse = client
        .post(format!("{}/api/v1/users/token", url))
        .form(&[
            ("client_id",     oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type",    "password"),
            ("response_type", "code"),
            ("username",      username),
            ("password",      password),
        ])
        .send().await?
        .json().await?;

    // 3. Find the user's default channel
    let channels: ChannelList = client
        .get(format!("{}/api/v1/accounts/{}/video-channels", url, username))
        .bearer_auth(&token.access_token)
        .send().await?
        .json().await?;

    let channel_id = channels.data.first()
        .ok_or_else(|| anyhow!("No video channel found for PeerTube user '{}'", username))?
        .id;

    // 4. Stream-upload the video file
    let filename = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video")
        .to_string();
    let title = Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&filename)
        .to_string();
    let mime = mime_for(file_path);

    let file = tokio::fs::File::open(file_path).await?;
    let file_size = file.metadata().await?.len();
    let stream = ReaderStream::new(file);
    let body = reqwest::Body::wrap_stream(stream);

    let video_part = reqwest::multipart::Part::stream_with_length(body, file_size)
        .file_name(filename)
        .mime_str(mime)?;

    let form = reqwest::multipart::Form::new()
        .text("name", title)
        .text("channelId", channel_id.to_string())
        .text("privacy", "1") // 1 = Public
        .part("videofile", video_part);

    let resp = client
        .post(format!("{}/api/v1/videos/upload", url))
        .bearer_auth(&token.access_token)
        .multipart(form)
        .send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("PeerTube upload failed ({}): {}", status, body));
    }

    Ok(())
}

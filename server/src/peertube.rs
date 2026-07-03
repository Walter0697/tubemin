use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
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
struct UserSearchResult {
    data: Vec<UserEntry>,
}

#[derive(Deserialize)]
struct UserEntry {
    username: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateUserBody<'a> {
    username: &'a str,
    password: &'a str,
    email: &'a str,
    role: u8,         // 0 = User
    video_quota: i64, // -1 = unlimited
    video_quota_daily: i64,
}

static BOT_AVATAR: &[u8] = include_bytes!("../assets/icon128.png");

/// Ensures the bot account exists in PeerTube, creating it if necessary.
/// Must be called with admin credentials; bot credentials are separate.
pub async fn ensure_account(
    url: &str,
    host_override: Option<&str>,
    admin_username: &str,
    admin_password: &str,
    bot_username: &str,
    bot_password: &str,
    bot_email: &str,
) -> Result<()> {
    let host = host_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));
    let client = Client::new();

    // Auth as admin
    let resp = client
        .get(format!("{}/api/v1/oauth-clients/local", url))
        .header("Host", &host)
        .send().await?;
    let body = resp.text().await?;
    let oauth: OAuthClient = serde_json::from_str(&body)
        .map_err(|e| anyhow!("oauth-clients parse error ({e}): {body}"))?;

    let resp = client
        .post(format!("{}/api/v1/users/token", url))
        .header("Host", &host)
        .form(&[
            ("client_id",     oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type",    "password"),
            ("response_type", "code"),
            ("username",      admin_username),
            ("password",      admin_password),
        ])
        .send().await?;
    let body = resp.text().await?;
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow!("admin token parse error ({e}): {body}"))?;

    // Check if bot account already exists
    let resp = client
        .get(format!("{}/api/v1/users?search={}&count=1", url, bot_username))
        .header("Host", &host)
        .bearer_auth(&token.access_token)
        .send().await?;
    let body = resp.text().await?;
    let results: UserSearchResult = serde_json::from_str(&body)
        .map_err(|e| anyhow!("user search parse error ({e}): {body}"))?;

    if results.data.iter().any(|u| u.username == bot_username) {
        tracing::info!("PeerTube bot account '{}' already exists", bot_username);
        return Ok(());
    }

    // Create the bot account
    let body = serde_json::to_string(&CreateUserBody {
        username: bot_username,
        password: bot_password,
        email: bot_email,
        role: 0,
        video_quota: -1,
        video_quota_daily: -1,
    })?;

    let resp = client
        .post(format!("{}/api/v1/users", url))
        .header("Host", &host)
        .header("Content-Type", "application/json")
        .bearer_auth(&token.access_token)
        .body(body)
        .send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Failed to create bot account '{}' ({}): {}", bot_username, status, body));
    }

    tracing::info!("Created PeerTube bot account '{}'", bot_username);

    // Set the bot's avatar — log in as bot, then upload icon
    if let Err(e) = set_bot_avatar(&client, url, &host, &oauth, bot_username, bot_password).await {
        tracing::warn!("Could not set bot avatar (non-fatal): {}", e);
    }

    Ok(())
}

async fn set_bot_avatar(
    client: &Client,
    url: &str,
    host: &str,
    oauth: &OAuthClient,
    bot_username: &str,
    bot_password: &str,
) -> Result<()> {
    let resp = client
        .post(format!("{}/api/v1/users/token", url))
        .header("Host", host)
        .form(&[
            ("client_id",     oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type",    "password"),
            ("response_type", "code"),
            ("username",      bot_username),
            ("password",      bot_password),
        ])
        .send().await?;
    let body = resp.text().await?;
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow!("bot token parse error ({e}): {body}"))?;

    let avatar_part = reqwest::multipart::Part::bytes(BOT_AVATAR)
        .file_name("icon128.png")
        .mime_str("image/png")?;
    let form = reqwest::multipart::Form::new().part("avatarfile", avatar_part);

    let resp = client
        .post(format!("{}/api/v1/users/me/avatar/pick", url))
        .header("Host", host)
        .bearer_auth(&token.access_token)
        .multipart(form)
        .send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("avatar upload failed ({}): {}", status, body));
    }

    tracing::info!("Set avatar for PeerTube bot account '{}'", bot_username);
    Ok(())
}

#[derive(Deserialize)]
struct ChannelList {
    data: Vec<VideoChannel>,
}

#[derive(Deserialize)]
struct UploadResponse {
    video: UploadedVideo,
}

#[derive(Deserialize)]
struct UploadedVideo {
    uuid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoDetails {
    preview_path: String,
}

/// Upload subtitle captions to PeerTube for an already-uploaded video.
/// `captions` is a list of (language_code, vtt_bytes), e.g. ("en", vtt_bytes).
pub async fn upload_captions(
    url: &str,
    host_override: Option<&str>,
    username: &str,
    password: &str,
    video_uuid: &str,
    captions: &[(String, Vec<u8>)],
) -> Result<()> {
    if captions.is_empty() { return Ok(()); }

    let host = host_override.map(|s| s.to_string()).unwrap_or_else(|| derive_host(url));
    let client = Client::new();

    let resp = client.get(format!("{}/api/v1/oauth-clients/local", url)).header("Host", &host).send().await?;
    let body = resp.text().await?;
    let oauth: OAuthClient = serde_json::from_str(&body)
        .map_err(|e| anyhow!("oauth-clients parse error ({e}): {body}"))?;

    let resp = client
        .post(format!("{}/api/v1/users/token", url))
        .header("Host", &host)
        .form(&[
            ("client_id", oauth.client_id.as_str()), ("client_secret", oauth.client_secret.as_str()),
            ("grant_type", "password"), ("response_type", "code"),
            ("username", username), ("password", password),
        ])
        .send().await?;
    let body = resp.text().await?;
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow!("token parse error ({e}): {body}"))?;

    for (lang, vtt_bytes) in captions {
        let part = reqwest::multipart::Part::bytes(vtt_bytes.clone())
            .file_name(format!("{}.vtt", lang))
            .mime_str("text/vtt")?;
        let form = reqwest::multipart::Form::new().part("captionfile", part);
        let resp = client
            .put(format!("{}/api/v1/videos/{}/captions/{}", url, video_uuid, lang))
            .header("Host", &host)
            .bearer_auth(&token.access_token)
            .multipart(form)
            .send().await?;
        if resp.status().is_success() {
            tracing::info!("Uploaded {} caption for video {}", lang, video_uuid);
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Caption upload failed for lang {} ({}): {}", lang, status, body);
        }
    }
    Ok(())
}

pub async fn delete_video(url: &str, host_override: Option<&str>, username: &str, password: &str, video_uuid: &str) -> Result<()> {
    let host = host_override.map(|s| s.to_string()).unwrap_or_else(|| derive_host(url));
    let client = Client::new();

    let resp = client.get(format!("{}/api/v1/oauth-clients/local", url)).header("Host", &host).send().await?;
    let oauth: OAuthClient = serde_json::from_str(&resp.text().await?)?;

    let resp = client
        .post(format!("{}/api/v1/users/token", url))
        .header("Host", &host)
        .form(&[
            ("client_id", oauth.client_id.as_str()), ("client_secret", oauth.client_secret.as_str()),
            ("grant_type", "password"), ("response_type", "code"),
            ("username", username), ("password", password),
        ])
        .send().await?;
    let token: TokenResponse = serde_json::from_str(&resp.text().await?)?;

    let resp = client
        .delete(format!("{}/api/v1/videos/{}", url, video_uuid))
        .header("Host", &host)
        .bearer_auth(&token.access_token)
        .send().await?;

    if !resp.status().is_success() && resp.status().as_u16() != 404 {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("PeerTube delete failed ({}): {}", status, body));
    }
    Ok(())
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

/// Upload a video to PeerTube. Returns the `/lazy-static/previews/{uuid}.jpg` path on success.
pub async fn upload(url: &str, host_override: Option<&str>, username: &str, password: &str, privacy: u8, file_path: &Path, meta: &crate::video_meta::VideoMeta, thumbnail: Option<(Vec<u8>, &str)>) -> Result<(String, String)> {
    // PeerTube validates Host against PEERTUBE_WEBSERVER_HOSTNAME (its public hostname).
    // When Tubemin connects via Docker-internal URL (peertube:9000) we must send the
    // public hostname (localhost:9000) in the Host header. PEERTUBE_HOST provides this.
    let host = host_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));

    let client = Client::new();

    // 1. Get OAuth client credentials
    let resp = client
        .get(format!("{}/api/v1/oauth-clients/local", url))
        .header("Host", &host)
        .send().await?;
    let body = resp.text().await?;
    let oauth: OAuthClient = serde_json::from_str(&body)
        .map_err(|e| anyhow!("oauth-clients parse error ({e}): {body}"))?;

    // 2. Exchange credentials for access token
    let resp = client
        .post(format!("{}/api/v1/users/token", url))
        .header("Host", &host)
        .form(&[
            ("client_id",     oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type",    "password"),
            ("response_type", "code"),
            ("username",      username),
            ("password",      password),
        ])
        .send().await?;
    let body = resp.text().await?;
    let token: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow!("token parse error ({e}): {body}"))?;

    // 3. Find the user's default channel
    let resp = client
        .get(format!("{}/api/v1/accounts/{}/video-channels", url, username))
        .header("Host", &host)
        .bearer_auth(&token.access_token)
        .send().await?;
    let body = resp.text().await?;
    let channels: ChannelList = serde_json::from_str(&body)
        .map_err(|e| anyhow!("video-channels parse error ({e}): {body}"))?;

    let channel_id = channels.data.first()
        .ok_or_else(|| anyhow!("No video channel found for PeerTube user '{}'", username))?
        .id;

    // 4. Stream-upload the video file
    let filename = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video")
        .to_string();
    let title = meta.title.clone().unwrap_or_else(|| {
        Path::new(&filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&filename)
            .to_string()
    });
    let mime = mime_for(file_path);

    let file = tokio::fs::File::open(file_path).await?;
    let file_size = file.metadata().await?.len();
    let stream = ReaderStream::new(file);
    let body = reqwest::Body::wrap_stream(stream);

    let video_part = reqwest::multipart::Part::stream_with_length(body, file_size)
        .file_name(filename)
        .mime_str(mime)?;

    let description = crate::video_meta::format_description(meta);

    let mut form = reqwest::multipart::Form::new()
        .text("name", title)
        .text("channelId", channel_id.to_string())
        .text("privacy", privacy.to_string());

    if !description.is_empty() {
        form = form.text("description", description);
    }
    if let Some(iso) = meta.upload_date.as_deref().and_then(crate::video_meta::upload_date_to_iso) {
        form = form.text("originallyPublishedAt", iso);
    }

    let mut form = form.part("videofile", video_part);

    if let Some((thumb_bytes, thumb_mime)) = thumbnail {
        let thumb1 = reqwest::multipart::Part::bytes(thumb_bytes.clone())
            .file_name("thumbnail.jpg")
            .mime_str(thumb_mime)?;
        let thumb2 = reqwest::multipart::Part::bytes(thumb_bytes)
            .file_name("preview.jpg")
            .mime_str(thumb_mime)?;
        form = form.part("thumbnailfile", thumb1).part("previewfile", thumb2);
    }

    let resp = client
        .post(format!("{}/api/v1/videos/upload", url))
        .header("Host", &host)
        .bearer_auth(&token.access_token)
        .multipart(form)
        .send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("PeerTube upload failed ({}): {}", status, body));
    }

    let body = resp.text().await?;
    let upload: UploadResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow!("upload response parse error ({e}): {body}"))?;

    // Fetch the full video record to get the actual previewPath — the upload
    // response only contains the video UUID, not the preview file's UUID.
    let details_resp = client
        .get(format!("{}/api/v1/videos/{}", url, upload.video.uuid))
        .header("Host", &host)
        .bearer_auth(&token.access_token)
        .send().await?;
    if details_resp.status().is_success() {
        let details_body = details_resp.text().await?;
        if let Ok(details) = serde_json::from_str::<VideoDetails>(&details_body) {
            return Ok((details.preview_path, upload.video.uuid));
        }
    }
    Ok((format!("/lazy-static/previews/{}.jpg", upload.video.uuid), upload.video.uuid))
}

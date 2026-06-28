use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

pub fn start(
    pool: Arc<SqlitePool>,
    pt_url: String,
    pt_host: Option<String>,
    pt_user: String,
    pt_pass: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            let rows: Vec<(String, String)> = match sqlx::query_as(
                "SELECT id, peertube_uuid FROM submissions WHERE peertube_uuid IS NOT NULL AND status IN ('imported', 'transcoding')"
            )
            .fetch_all(pool.as_ref())
            .await {
                Ok(r) => r,
                Err(e) => { error!(error = %e, "transcoding poller db error"); continue; }
            };

            if rows.is_empty() { continue; }

            let token = match fetch_token(&pt_url, pt_host.as_deref(), &pt_user, &pt_pass).await {
                Ok(t) => t,
                Err(e) => { warn!(error = %e, "transcoding poller: could not get PeerTube token"); continue; }
            };

            for (sub_id, uuid) in rows {
                match fetch_video_state(&pt_url, pt_host.as_deref(), &token, &uuid).await {
                    Ok(state_id) => {
                        if state_id == 1 {
                            if let Err(e) = crate::db::mark_complete(&pool, &uuid).await {
                                error!(error = %e, uuid = %uuid, "transcoding poller: mark_complete error");
                            } else {
                                info!(sub_id = %sub_id, uuid = %uuid, "video transcoding complete");
                            }
                        } else {
                            if let Err(e) = crate::db::mark_transcoding(&pool, &uuid).await {
                                error!(error = %e, uuid = %uuid, "transcoding poller: mark_transcoding error");
                            }
                        }
                    }
                    Err(e) => warn!(error = %e, uuid = %uuid, "transcoding poller: could not fetch video state"),
                }
            }
        }
    })
}

async fn fetch_token(url: &str, host: Option<&str>, username: &str, password: &str) -> anyhow::Result<String> {
    use serde::Deserialize;
    #[derive(Deserialize)] struct OAuthClient { client_id: String, client_secret: String }
    #[derive(Deserialize)] struct TokenResp { access_token: String }

    let h = host.map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));
    let client = reqwest::Client::new();

    let body = client.get(format!("{}/api/v1/oauth-clients/local", url))
        .header("Host", &h).send().await?.text().await?;
    let oauth: OAuthClient = serde_json::from_str(&body)?;

    let body = client.post(format!("{}/api/v1/users/token", url))
        .header("Host", &h)
        .form(&[
            ("client_id", oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.as_str()),
            ("grant_type", "password"),
            ("response_type", "code"),
            ("username", username),
            ("password", password),
        ])
        .send().await?.text().await?;
    let token: TokenResp = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("token parse error ({e}): {body}"))?;
    Ok(token.access_token)
}

async fn fetch_video_state(url: &str, host: Option<&str>, token: &str, uuid: &str) -> anyhow::Result<u64> {
    use serde::Deserialize;
    #[derive(Deserialize)] struct State { id: u64 }
    #[derive(Deserialize)] struct Video { state: State }

    let h = host.map(|s| s.to_string())
        .unwrap_or_else(|| derive_host(url));
    let client = reqwest::Client::new();

    let resp = client.get(format!("{}/api/v1/videos/{}", url, uuid))
        .header("Host", &h)
        .bearer_auth(token)
        .send().await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("PeerTube returned {}", resp.status()));
    }
    let body = resp.text().await?;
    let video: Video = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("video parse error ({e}): {body}"))?;
    Ok(video.state.id)
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

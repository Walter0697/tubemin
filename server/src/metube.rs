use reqwest::Client;
use thiserror::Error;
use serde_json::json;

#[derive(Debug, Error)]
pub enum MeTubeError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("metube returned status {0}")]
    BadStatus(u16),
}

pub struct QueueItem {
    pub url: String,
    pub title: Option<String>,
}

pub struct QueueState {
    /// URLs MeTube is actively downloading
    pub active: Vec<QueueItem>,
    /// URLs waiting to start in MeTube's queue
    pub pending: Vec<QueueItem>,
    /// URLs MeTube finished with an error
    pub errored: Vec<QueueItem>,
}

fn extract_items(arr: Option<&serde_json::Value>, error_filter: bool) -> Vec<QueueItem> {
    arr.and_then(|v| v.as_array())
        .map(|items| {
            items.iter().filter_map(|item| {
                if error_filter && item["error"].is_null() {
                    return None;
                }
                let url = item["url"].as_str()?.to_string();
                let title = item["title"].as_str().map(str::to_string);
                Some(QueueItem { url, title })
            }).collect()
        })
        .unwrap_or_default()
}

pub async fn get_queue_state(metube_url: &str) -> Result<QueueState, MeTubeError> {
    let client = Client::new();
    let resp = client
        .get(format!("{}/history", metube_url))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(MeTubeError::BadStatus(resp.status().as_u16()));
    }

    let data: serde_json::Value = resp.json().await?;
    Ok(QueueState {
        active:  extract_items(data.get("queue"),   false),
        pending: extract_items(data.get("pending"), false),
        errored: extract_items(data.get("done"),    true),
    })
}

pub async fn submit(metube_url: &str, url: &str) -> Result<(), MeTubeError> {
    let client = Client::new();
    let resp = client
        .post(format!("{}/add", metube_url))
        .json(&json!({
            "url": url,
            "folder": "/downloads",
            "auto_start": true
        }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(MeTubeError::BadStatus(status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path, body_json};
    use serde_json::json;

    #[tokio::test]
    async fn posts_to_metube_add() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .and(body_json(json!({"url": "https://example.com/video", "folder": "/downloads", "auto_start": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
            .mount(&server)
            .await;

        let result = submit(&server.uri(), "https://example.com/video").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn returns_error_on_bad_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/add"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let result = submit(&server.uri(), "https://example.com/video").await;
        assert!(matches!(result, Err(MeTubeError::BadStatus(500))));
    }
}

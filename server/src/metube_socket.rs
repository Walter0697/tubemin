use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::time::Duration;
use tracing::{info, warn};
use crate::progress::ProgressMap;

pub fn start(metube_url: String, pool: Arc<SqlitePool>, progress: ProgressMap) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = run(&metube_url, &pool, &progress).await {
                warn!(error = %e, "MeTube socket error, reconnecting in 5s");
            } else {
                warn!("MeTube socket closed, reconnecting in 5s");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn run(metube_url: &str, pool: &SqlitePool, progress: &ProgressMap) -> anyhow::Result<()> {
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    use futures_util::{SinkExt, StreamExt};

    // Engine.IO v4 polling handshake to get session ID
    let body = reqwest::get(format!("{}/socket.io/?EIO=4&transport=polling", metube_url))
        .await?.text().await?;
    if !body.starts_with('0') {
        return Err(anyhow::anyhow!("unexpected EIO open packet: {}", body));
    }
    let sid = serde_json::from_str::<serde_json::Value>(&body[1..])?["sid"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no sid in EIO open packet"))?
        .to_string();

    let ws_url = format!(
        "{}/socket.io/?EIO=4&transport=websocket&sid={}",
        metube_url.replace("http://", "ws://").replace("https://", "wss://"),
        sid
    );
    let (mut ws, _) = connect_async(&ws_url).await?;

    // Engine.IO probe + upgrade to WebSocket transport
    ws.send(Message::Text("2probe".into())).await?;
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) if t == "3probe" => break,
            Some(Ok(_)) => {}
            _ => return Err(anyhow::anyhow!("EIO probe failed")),
        }
    }
    ws.send(Message::Text("5".into())).await?;

    // Socket.IO default namespace connect
    ws.send(Message::Text("40".into())).await?;

    info!("connected to MeTube Socket.IO");

    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(t) if t == "2" => {
                // Engine.IO ping → pong
                ws.send(Message::Text("3".into())).await?;
            }
            Message::Text(t) if t.starts_with("42") => {
                if let Ok(arr) = serde_json::from_str::<serde_json::Value>(&t[2..]) {
                    match arr[0].as_str() {
                        Some("updated") | Some("added") => {
                            handle_item(arr.get(1).unwrap_or(&serde_json::Value::Null), pool, progress).await;
                        }
                        Some("completed") | Some("error") => {
                            if let Some(url) = arr.get(1).and_then(|d| d["url"].as_str()) {
                                if let Ok(Some(sub)) = crate::db::get_submission_by_url(pool, url).await {
                                    crate::progress::remove(progress, &sub.id);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    Ok(())
}

async fn handle_item(data: &serde_json::Value, pool: &SqlitePool, progress: &ProgressMap) {
    let url = match data["url"].as_str() {
        Some(u) => u,
        None => return,
    };
    let pct = match data["percent"].as_f64() {
        Some(p) => p,
        None => return,
    };
    if let Ok(Some(sub)) = crate::db::get_submission_by_url(pool, url).await {
        crate::progress::set(progress, &sub.id, (pct / 100.0) as f32);
    }
}

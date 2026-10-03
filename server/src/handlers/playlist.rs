use crate::oidc::RequireAuth;
use crate::state::AppState;
use crate::{db, url_validator};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Upper bound on entries shown in the picker. Also caps endless YouTube Mix
/// ("RD…") lists, which yt-dlp would otherwise keep expanding.
pub const MAX_PLAYLIST_ENTRIES: usize = 50;
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
pub struct PlaylistPreviewRequest {
    pub url: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PlaylistEntry {
    pub video_id: String,
    pub url: String,
    pub title: String,
    pub duration: Option<u64>,
    pub thumbnail_url: String,
    pub available: bool,
    /// `active`, `transferred`, `failed`, or null when Tubemin has never seen it.
    pub existing_status: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct PlaylistPreview {
    pub playlist_id: String,
    pub title: Option<String>,
    pub selected_video_id: Option<String>,
    pub truncated: bool,
    pub entries: Vec<PlaylistEntry>,
}

/// Groups a submission status the way the picker presents it.
pub fn existing_status(status: &str) -> Option<&'static str> {
    match status {
        "pending" | "downloading" | "imported" | "transcoding" | "complete" => Some("active"),
        "processing" | "deleted" | "handed_off" => Some("transferred"),
        "error" | "interrupted" => Some("failed"),
        _ => None,
    }
}

fn entry_available(entry: &Value) -> bool {
    let title = entry["title"].as_str().unwrap_or("");
    if matches!(title, "[Private video]" | "[Deleted video]" | "[Unavailable video]") {
        return false;
    }
    !matches!(
        entry["availability"].as_str(),
        Some("private" | "premium_only" | "subscriber_only" | "needs_auth")
    )
}

/// Builds the picker payload from `yt-dlp --flat-playlist --dump-single-json`.
pub fn parse_flat_playlist(
    data: &Value,
    playlist_id: &str,
    selected_video_id: Option<String>,
) -> PlaylistPreview {
    let raw_entries = data["entries"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let entries: Vec<PlaylistEntry> = raw_entries
        .iter()
        .filter_map(|entry| {
            let video_id = entry["id"].as_str()?.to_string();
            if url_validator::youtube_video_id(&url_validator::canonical_youtube_url(&video_id))
                .as_deref()
                != Some(video_id.as_str())
            {
                return None;
            }
            Some(PlaylistEntry {
                url: url_validator::canonical_youtube_url(&video_id),
                title: entry["title"].as_str().unwrap_or(&video_id).to_string(),
                duration: entry["duration"].as_f64().map(|d| d.round() as u64),
                thumbnail_url: format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"),
                available: entry_available(entry),
                existing_status: None,
                video_id,
            })
        })
        .take(MAX_PLAYLIST_ENTRIES)
        .collect();
    let truncated = match data["playlist_count"].as_u64() {
        Some(count) => count as usize > entries.len(),
        None => raw_entries.len() >= MAX_PLAYLIST_ENTRIES,
    };
    PlaylistPreview {
        playlist_id: playlist_id.to_string(),
        title: data["title"].as_str().map(str::to_string),
        selected_video_id,
        truncated,
        entries,
    }
}

pub fn apply_existing_statuses(entries: &mut [PlaylistEntry], statuses: &HashMap<String, String>) {
    for entry in entries {
        entry.existing_status = statuses
            .get(&format!("youtube:{}", entry.video_id))
            .and_then(|status| existing_status(status));
    }
}

async fn run_yt_dlp(url: &str) -> Result<Value, String> {
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args([
        "--flat-playlist",
        "--yes-playlist",
        "--dump-single-json",
        "--no-warnings",
        "--no-cache-dir",
        "--playlist-end",
        &MAX_PLAYLIST_ENTRIES.to_string(),
        "--",
        url,
    ])
    .kill_on_drop(true);
    let output = tokio::time::timeout(PREVIEW_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "timed out reading the playlist".to_string())?
        .map_err(|e| format!("could not run yt-dlp: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("yt-dlp failed");
        return Err(message.trim().chars().take(300).collect());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("unexpected yt-dlp output: {e}"))
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Dashboard playlist picker: lists up to 50 playlist entries and flags the
/// ones Tubemin already has. Nothing is queued here.
pub async fn playlist_preview(
    RequireAuth(_user): RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<PlaylistPreviewRequest>,
) -> Response {
    let Some(playlist_id) = url_validator::youtube_playlist_id(&body.url) else {
        return error(StatusCode::BAD_REQUEST, "URL is not a YouTube playlist");
    };
    let selected = url_validator::youtube_video_id(&body.url);
    let data = match run_yt_dlp(body.url.trim()).await {
        Ok(data) => data,
        Err(message) => {
            tracing::warn!(url = %body.url, error = %message, "playlist preview failed");
            return error(StatusCode::BAD_GATEWAY, &message);
        }
    };
    let mut preview = parse_flat_playlist(&data, &playlist_id, selected);
    let keys: Vec<String> = preview
        .entries
        .iter()
        .map(|entry| format!("youtube:{}", entry.video_id))
        .collect();
    match db::latest_status_by_video_keys(&state.pool, &keys).await {
        Ok(statuses) => apply_existing_statuses(&mut preview.entries, &statuses),
        Err(e) => {
            tracing::error!(error = %e, "db error loading playlist entry statuses");
            return error(StatusCode::INTERNAL_SERVER_ERROR, "db error");
        }
    }
    Json(preview).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "id": "PL123",
            "title": "Road trip",
            "playlist_count": 3,
            "entries": [
                {"id": "aaaaaaaaaaa", "title": "First", "duration": 201.4},
                {"id": "bbbbbbbbbbb", "title": "[Private video]", "duration": null},
                {"id": "ccccccccccc", "title": "Members", "availability": "subscriber_only"}
            ]
        })
    }

    #[test]
    fn parses_entries_into_canonical_single_video_urls() {
        let preview = parse_flat_playlist(&sample(), "PL123", Some("aaaaaaaaaaa".into()));
        assert_eq!(preview.title.as_deref(), Some("Road trip"));
        assert_eq!(preview.selected_video_id.as_deref(), Some("aaaaaaaaaaa"));
        assert!(!preview.truncated);
        assert_eq!(preview.entries.len(), 3);
        let first = &preview.entries[0];
        assert_eq!(first.url, "https://www.youtube.com/watch?v=aaaaaaaaaaa");
        assert_eq!(first.duration, Some(201));
        assert_eq!(first.thumbnail_url, "https://i.ytimg.com/vi/aaaaaaaaaaa/mqdefault.jpg");
        assert!(first.available);
        assert!(!preview.entries[1].available);
        assert!(!preview.entries[2].available);
    }

    #[test]
    fn marks_truncated_when_playlist_is_larger_than_the_cap() {
        let mut data = sample();
        data["playlist_count"] = json!(120);
        assert!(parse_flat_playlist(&data, "PL123", None).truncated);
    }

    #[test]
    fn marks_mix_without_count_truncated_at_the_cap() {
        let entries: Vec<Value> = (0..MAX_PLAYLIST_ENTRIES)
            .map(|i| json!({"id": format!("vid{i:08}"), "title": format!("Song {i}")}))
            .collect();
        let data = json!({"id": "RDabc", "entries": entries});
        let preview = parse_flat_playlist(&data, "RDabc", None);
        assert!(preview.truncated);
        assert_eq!(preview.entries.len(), MAX_PLAYLIST_ENTRIES);
    }

    #[test]
    fn skips_entries_without_a_valid_video_id() {
        let data = json!({"entries": [{"title": "no id"}, {"id": "bad id", "title": "x"}]});
        assert!(parse_flat_playlist(&data, "PL1", None).entries.is_empty());
    }

    #[test]
    fn maps_submission_statuses_to_picker_states() {
        let mut preview = parse_flat_playlist(&sample(), "PL123", None);
        let statuses = HashMap::from([
            ("youtube:aaaaaaaaaaa".to_string(), "transcoding".to_string()),
            ("youtube:bbbbbbbbbbb".to_string(), "handed_off".to_string()),
            ("youtube:ccccccccccc".to_string(), "interrupted".to_string()),
        ]);
        apply_existing_statuses(&mut preview.entries, &statuses);
        let states: Vec<_> = preview.entries.iter().map(|e| e.existing_status).collect();
        assert_eq!(states, vec![Some("active"), Some("transferred"), Some("failed")]);
        assert_eq!(existing_status("deleted"), Some("transferred"));
        assert_eq!(existing_status("complete"), Some("active"));
        assert_eq!(existing_status("unknown"), None);
    }
}

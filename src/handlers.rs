// src/handlers.rs - Clean version without WebSocket, fixed imports

use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::services::playlist;
use crate::services::streamer::StreamManager;
use crate::config;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "ChillOut Radio - Direct Streaming",
    })
}

#[get("/api/now-playing")]
pub async fn now_playing(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Get the actual current track from the stream manager's state
    let track_info = sm.get_track_info();
    let playback_position = sm.get_playback_position();
    let active_listeners = sm.get_active_listeners();
    let current_bitrate = sm.get_current_bitrate();
    
    // If we have track info from the stream manager, parse and use it
    if let Some(track_json) = track_info {
        if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(&track_json) {
            if let serde_json::Value::Object(ref mut map) = track_value {
                map.insert(
                    "active_listeners".to_string(), 
                    serde_json::Value::Number(serde_json::Number::from(active_listeners))
                );
                map.insert(
                    "playback_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(playback_position))
                );
                map.insert(
                    "bitrate".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(current_bitrate / 1000))
                );
            }
            return Json(track_value);
        }
    }
    
    // Fallback to playlist if stream manager doesn't have current info
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    match track {
        Some(track) => {
            let mut track_json = serde_json::to_value(track).unwrap_or_default();
            if let serde_json::Value::Object(ref mut map) = track_json {
                map.insert(
                    "active_listeners".to_string(), 
                    serde_json::Value::Number(serde_json::Number::from(active_listeners))
                );
                map.insert(
                    "playback_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(playback_position))
                );
                map.insert(
                    "bitrate".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(current_bitrate / 1000))
                );
            }
            
            Json(track_json)
        },
        None => Json(serde_json::json!({
            "error": "No tracks available"
        }))
    }
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Collect stats
    let active_listeners = sm.get_active_listeners();
    let is_streaming = sm.is_streaming();
    let track_ended = sm.track_ended();
    let current_bitrate = sm.get_current_bitrate();
    let playback_position = sm.get_playback_position();
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
        "max_concurrent_users": config::MAX_CONCURRENT_USERS,
        "streaming": is_streaming,
        "track_ended": track_ended,
        "bitrate_kbps": current_bitrate / 1000,
        "playback_position": playback_position,
        "streaming_method": "direct_chunked",
        "server_time": chrono::Local::now().to_rfc3339()
    }))
}

#[get("/diag")]
pub async fn diagnostic_page() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/diag.html")).await.ok()
}

// Helper function to serve static files
#[get("/static/<file..>")]
pub async fn static_files(file: PathBuf) -> Option<NamedFile> {
    let path = Path::new("static/").join(file);
    NamedFile::open(path).await.ok()
}

// Error catchers
#[catch(404)]
pub async fn not_found() -> Template {
    Template::render("error", context! {
        status: 404,
        message: "Page not found"
    })
}

#[catch(500)]
pub async fn server_error() -> Template {
    Template::render("error", context! {
        status: 500,
        message: "Internal server error"
    })
}

#[catch(503)]
pub async fn service_unavailable() -> Template {
    Template::render("error", context! {
        status: 503,
        message: "Server at capacity, try again later"
    })
}
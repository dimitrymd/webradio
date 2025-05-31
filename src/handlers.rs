// src/handlers.rs - Complete radio handlers

use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::services::streamer::StreamManager;
use crate::services::playlist;
use crate::config;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "ChillOut Radio - Live Radio Stream",
        version: "2.3.0-complete-radio"
    })
}

#[get("/api/now-playing?<mobile_client>&<android_client>")]
pub async fn now_playing(
    mobile_client: Option<bool>,
    android_client: Option<bool>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    sm.cleanup_stale_connections();
    
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    let is_mobile = mobile_client.unwrap_or(false) || android_client.unwrap_or(false);
    
    // Get track info from stream manager
    if let Some(track_json) = &track_state.track_info {
        if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(track_json) {
            if let serde_json::Value::Object(ref mut map) = track_value {
                // Radio position (same for all clients)
                map.insert(
                    "radio_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.position_seconds))
                );
                map.insert(
                    "radio_position_ms".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.position_milliseconds))
                );
                
                // Legacy compatibility
                map.insert(
                    "playback_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.position_seconds))
                );
                map.insert(
                    "playback_position_ms".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.position_milliseconds))
                );
                
                map.insert(
                    "active_listeners".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(active_listeners))
                );
                map.insert(
                    "bitrate".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.bitrate / 1000))
                );
                map.insert(
                    "server_timestamp".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64
                    ))
                );
                
                // Radio metadata
                map.insert("streaming_mode".to_string(), serde_json::Value::String("radio".to_string()));
                map.insert("seeking_enabled".to_string(), serde_json::Value::Bool(false));
                map.insert("synchronized_playback".to_string(), serde_json::Value::Bool(true));
                
                if is_mobile {
                    map.insert("mobile_optimized".to_string(), serde_json::Value::Bool(true));
                }
                
                map.insert("streaming".to_string(), serde_json::Value::Bool(sm.is_streaming()));
                map.insert("track_ended".to_string(), serde_json::Value::Bool(sm.track_ended()));
            }
            return Json(track_value);
        }
    }
    
    // Fallback response
    Json(serde_json::json!({
        "title": "ChillOut Radio",
        "artist": "Live Stream",
        "album": "Now Playing",
        "duration": track_state.duration,
        "path": "live.mp3",
        "radio_position": track_state.position_seconds,
        "radio_position_ms": track_state.position_milliseconds,
        "playback_position": track_state.position_seconds,
        "playback_position_ms": track_state.position_milliseconds,
        "active_listeners": active_listeners,
        "bitrate": track_state.bitrate / 1000,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "streaming_mode": "radio",
        "seeking_enabled": false,
        "synchronized_playback": true,
        "streaming": sm.is_streaming(),
        "mobile_optimized": is_mobile
    }))
}

#[get("/api/heartbeat?<connection_id>")]
pub async fn heartbeat(
    connection_id: Option<String>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    sm.cleanup_stale_connections();
    
    if let Some(conn_id) = connection_id {
        sm.update_listener_heartbeat(&conn_id);
    }
    
    let active_listeners = sm.get_active_listeners();
    let (position_secs, position_ms) = sm.get_precise_position();
    
    Json(serde_json::json!({
        "status": "ok",
        "active_listeners": active_listeners,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "radio_position": position_secs,
        "radio_position_ms": position_ms,
        "streaming": sm.is_streaming(),
        "streaming_mode": "radio"
    }))
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    sm.cleanup_stale_connections();
    
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
        "streaming": sm.is_streaming(),
        "track_ended": sm.track_ended(),
        "bitrate_kbps": track_state.bitrate / 1000,
        "radio_position": track_state.position_seconds,
        "radio_position_ms": track_state.position_milliseconds,
        "track_duration": track_state.duration,
        "streaming_mode": "radio",
        "seeking_enabled": false,
        "synchronized_playback": true,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }))
}

#[get("/api/position")]
pub async fn get_position(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    
    Json(serde_json::json!({
        "radio_position": track_state.position_seconds,
        "radio_position_ms": track_state.position_milliseconds,
        "duration": track_state.duration,
        "remaining_time": track_state.remaining_time,
        "streaming_mode": "radio",
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }))
}

#[get("/api/playlist")]
pub async fn get_playlist() -> Json<serde_json::Value> {
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    let current_track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    Json(serde_json::json!({
        "tracks": playlist_data.tracks,
        "total_tracks": playlist_data.tracks.len(),
        "current_track": current_track,
        "streaming_mode": "radio",
        "seeking_enabled": false
    }))
}

// Static files
#[get("/static/<file..>")]
pub async fn static_files(file: PathBuf) -> Option<NamedFile> {
    let path = Path::new("static/").join(file);
    NamedFile::open(path).await.ok()
}

// Diagnostic page
#[get("/diag")]
pub async fn diagnostic_page() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/diag.html")).await.ok()
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
        message: "Server error"
    })
}
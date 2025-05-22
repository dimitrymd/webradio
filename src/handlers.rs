// src/handlers.rs - Complete enhanced version with precise position tracking

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
        title: "ChillOut Radio - Enhanced Position Sync",
    })
}

#[get("/api/now-playing")]
pub async fn now_playing(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Get comprehensive track state with precise timing
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    
    // Get track info from stream manager's state
    if let Some(track_json) = &track_state.track_info {
        if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(track_json) {
            if let serde_json::Value::Object(ref mut map) = track_value {
                // Enhanced position information
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
                    "is_near_end".to_string(),
                    serde_json::Value::Bool(track_state.is_near_end)
                );
                map.insert(
                    "remaining_time".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(track_state.remaining_time))
                );
                // Server timestamp for client synchronization
                map.insert(
                    "server_timestamp".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64
                    ))
                );
            }
            return Json(track_value);
        }
    }
    
    // Fallback to playlist if stream manager doesn't have current info
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    match track {
        Some(track) => {
            let (position_secs, position_ms) = sm.get_precise_position();
            
            let mut track_json = serde_json::to_value(track).unwrap_or_default();
            if let serde_json::Value::Object(ref mut map) = track_json {
                map.insert(
                    "playback_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(position_secs))
                );
                map.insert(
                    "playback_position_ms".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(position_ms))
                );
                map.insert(
                    "active_listeners".to_string(), 
                    serde_json::Value::Number(serde_json::Number::from(active_listeners))
                );
                map.insert(
                    "bitrate".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(sm.get_current_bitrate() / 1000))
                );
                map.insert(
                    "is_near_end".to_string(),
                    serde_json::Value::Bool(sm.is_near_track_end(10))
                );
                map.insert(
                    "remaining_time".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(sm.get_remaining_time()))
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
            }
            
            Json(track_json)
        },
        None => Json(serde_json::json!({
            "error": "No tracks available",
            "server_timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        }))
    }
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Get comprehensive streaming statistics
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    let is_streaming = sm.is_streaming();
    let track_ended = sm.track_ended();
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
        "max_concurrent_users": config::MAX_CONCURRENT_USERS,
        "streaming": is_streaming,
        "track_ended": track_ended,
        "bitrate_kbps": track_state.bitrate / 1000,
        "playback_position": track_state.position_seconds,
        "playback_position_ms": track_state.position_milliseconds,
        "track_duration": track_state.duration,
        "remaining_time": track_state.remaining_time,
        "is_near_track_end": track_state.is_near_end,
        "streaming_method": "enhanced_position_sync",
        "position_accuracy": "millisecond",
        "server_time": chrono::Local::now().to_rfc3339(),
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "features": {
            "position_persistence": true,
            "millisecond_precision": true,
            "drift_correction": true,
            "ios_optimized": true,
            "continuity_on_reconnect": true
        }
    }))
}

// Enhanced API endpoint for detailed position information
#[get("/api/position")]
pub async fn get_position(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    
    Json(serde_json::json!({
        "position_seconds": track_state.position_seconds,
        "position_milliseconds": track_state.position_milliseconds,
        "duration": track_state.duration,
        "remaining_time": track_state.remaining_time,
        "progress_percentage": if track_state.duration > 0 {
            (track_state.position_seconds as f64 / track_state.duration as f64) * 100.0
        } else {
            0.0
        },
        "is_near_end": track_state.is_near_end,
        "bitrate": track_state.bitrate,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }))
}

// API endpoint for client position sync verification
#[get("/api/sync-check?<client_position>&<client_timestamp>")]
pub async fn sync_check(
    client_position: Option<u64>,
    client_timestamp: Option<u64>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    let server_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    
    let mut response = serde_json::json!({
        "server_position": track_state.position_seconds,
        "server_position_ms": track_state.position_milliseconds,
        "server_timestamp": server_timestamp,
        "track_duration": track_state.duration
    });
    
    // Calculate drift if client provided position and timestamp
    if let (Some(client_pos), Some(client_ts)) = (client_position, client_timestamp) {
        let time_diff = (server_timestamp as i64 - client_ts as i64) / 1000; // seconds
        let expected_client_pos = (client_pos as i64 + time_diff) as u64;
        let server_pos = track_state.position_seconds;
        let drift = server_pos as i64 - expected_client_pos as i64;
        
        if let serde_json::Value::Object(ref mut map) = response {
            map.insert("client_position".to_string(), serde_json::Value::Number(serde_json::Number::from(client_pos)));
            map.insert("client_timestamp".to_string(), serde_json::Value::Number(serde_json::Number::from(client_ts)));
            map.insert("time_diff_ms".to_string(), serde_json::Value::Number(serde_json::Number::from(server_timestamp as i64 - client_ts as i64)));
            map.insert("expected_client_position".to_string(), serde_json::Value::Number(serde_json::Number::from(expected_client_pos)));
            map.insert("position_drift_seconds".to_string(), serde_json::Value::Number(serde_json::Number::from(drift)));
            map.insert("drift_significant".to_string(), serde_json::Value::Bool(drift.abs() > 3));
        }
    }
    
    Json(response)
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
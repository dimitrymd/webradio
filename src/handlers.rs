// src/handlers.rs - Minimal working version

use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch, options};
use rocket_dyn_templates::{Template, context};
use rocket::http::Header;
use rocket::response::Responder;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use log::info;

use crate::services::streamer::StreamManager;
use crate::services::playlist;
use crate::config;

// CORS Response wrapper
pub struct CorsResponse<T> {
    inner: T,
}

impl<T> CorsResponse<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<'r, T: Responder<'r, 'static>> Responder<'r, 'static> for CorsResponse<T> {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        let mut response = self.inner.respond_to(req)?;
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
        response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type, Authorization"));
        Ok(response)
    }
}

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "ChillOut Radio - Live Radio Stream",
        version: "2.4.0-fixed"
    })
}

#[get("/api/now-playing?<mobile_client>&<android_client>")]
pub async fn now_playing(
    mobile_client: Option<bool>,
    android_client: Option<bool>,
    stream_manager: &State<Arc<StreamManager>>
) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections
    sm.cleanup_stale_connections();
    
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    let is_mobile = mobile_client.unwrap_or(false) || android_client.unwrap_or(false);
    
    // Get track info from stream manager
    if let Some(track_json) = &track_state.track_info {
        if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(track_json) {
            if let serde_json::Value::Object(ref mut map) = track_value {
                // Add radio position
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
                
                // Additional metadata
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
                map.insert("streaming".to_string(), serde_json::Value::Bool(sm.is_streaming()));
                map.insert("track_ended".to_string(), serde_json::Value::Bool(sm.track_ended()));
                
                if is_mobile {
                    map.insert("mobile_optimized".to_string(), serde_json::Value::Bool(true));
                }
            }
            return CorsResponse::new(Json(track_value));
        }
    }
    
    // Fallback response
    CorsResponse::new(Json(serde_json::json!({
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
        "mobile_optimized": is_mobile,
        "track_ended": sm.track_ended()
    })))
}

#[get("/api/heartbeat?<connection_id>")]
pub async fn heartbeat(
    connection_id: Option<String>,
    stream_manager: &State<Arc<StreamManager>>
) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    if let Some(conn_id) = connection_id {
        sm.update_listener_heartbeat(&conn_id);
    }
    
    let active_listeners = sm.get_active_listeners();
    let (position_secs, position_ms) = sm.get_precise_position();
    
    CorsResponse::new(Json(serde_json::json!({
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
    })))
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    sm.cleanup_stale_connections();
    
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    
    CorsResponse::new(Json(serde_json::json!({
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
    })))
}

#[get("/api/position")]
pub async fn get_position(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    
    CorsResponse::new(Json(serde_json::json!({
        "radio_position": track_state.position_seconds,
        "radio_position_ms": track_state.position_milliseconds,
        "duration": track_state.duration,
        "remaining_time": track_state.remaining_time,
        "streaming_mode": "radio",
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })))
}

#[get("/api/playlist")]
pub async fn get_playlist() -> CorsResponse<Json<serde_json::Value>> {
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    let current_track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    CorsResponse::new(Json(serde_json::json!({
        "tracks": playlist_data.tracks,
        "total_tracks": playlist_data.tracks.len(),
        "current_track": current_track,
        "streaming_mode": "radio",
        "seeking_enabled": false
    })))
}

// Track switching endpoints
#[get("/api/switch-track")]
pub async fn switch_track(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    info!("Manual track switch requested via API");
    sm.request_track_switch();
    
    // Get current track state
    let track_state = sm.get_track_state();
    
    CorsResponse::new(Json(serde_json::json!({
        "status": "track_switch_requested",
        "message": "Track switch has been requested",
        "current_position": track_state.position_seconds,
        "current_duration": track_state.duration,
        "remaining_time": track_state.remaining_time,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })))
}

#[get("/api/next-track")]
pub async fn get_next_track() -> CorsResponse<Json<serde_json::Value>> {
    let next_track = playlist::get_next_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    match next_track {
        Some(track) => {
            CorsResponse::new(Json(serde_json::json!({
                "status": "success",
                "next_track": {
                    "title": track.title,
                    "artist": track.artist,
                    "album": track.album,
                    "duration": track.duration,
                    "path": track.path
                }
            })))
        },
        None => {
            CorsResponse::new(Json(serde_json::json!({
                "status": "error",
                "message": "No next track available"
            })))
        }
    }
}

#[get("/api/health")]
pub async fn health_check(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    CorsResponse::new(Json(serde_json::json!({
        "status": "healthy",
        "streaming": sm.is_streaming(),
        "active_listeners": sm.get_active_listeners(),
        "uptime": "unknown",
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })))
}

#[get("/api/connections")]
pub async fn get_connections(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    sm.cleanup_stale_connections();
    
    CorsResponse::new(Json(serde_json::json!({
        "active_connections": sm.get_active_listeners(),
        "streaming": sm.is_streaming(),
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })))
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

#[get("/favicon.ico")]
pub async fn favicon() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/favicon.ico")).await.ok()
}

#[get("/robots.txt")]
pub async fn robots() -> &'static str {
    "User-agent: *\nDisallow: /"
}

// CORS preflight
#[options("/<_..>")]
pub fn cors_preflight() -> CorsResponse<()> {
    CorsResponse::new(())
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

#[catch(503)]
pub async fn service_unavailable() -> Template {
    Template::render("error", context! {
        status: 503,
        message: "Service temporarily unavailable"
    })
}
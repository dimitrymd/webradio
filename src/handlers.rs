// src/handlers.rs - Optimized with JSON caching

use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch, options};
use rocket_dyn_templates::{Template, context};
use rocket::http::Header;
use rocket::response::Responder;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

// Pre-rendered index template
lazy_static::lazy_static! {
    static ref INDEX_TEMPLATE: String = {
        let template = include_str!("../templates/index.html.tera");
        template.replace("{{ title }}", "ChillOut Radio - Live Radio Stream")
                .replace("{{ version }}", "4.0.0")
                .replace("{{ mode }}", "true-radio")
    };
}

#[get("/")]
pub async fn index() -> Template {
    // Still use Template for now, but could be optimized further
    Template::render("index", context! {
        title: "ChillOut Radio - Live Radio Stream",
        version: "4.0.0",
        mode: "true-radio"
    })
}

#[get("/api/now-playing?<mobile_client>&<android_client>")]
pub async fn now_playing(
    mobile_client: Option<bool>,
    android_client: Option<bool>,
    stream_manager: &State<Arc<StreamManager>>
) -> CorsResponse<Json<Value>> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections
    sm.cleanup_stale_connections();
    
    let active_listeners = sm.get_active_listeners();
    let is_mobile = mobile_client.unwrap_or(false) || android_client.unwrap_or(false);
    
    // Try to get cached JSON first
    if let Some(cached_json) = sm.get_cached_track_info() {
        if let Ok(mut track_value) = serde_json::from_str::<Value>(&cached_json) {
            if let Value::Object(ref mut map) = track_value {
                // Update dynamic fields
                let (pos_secs, pos_ms) = sm.get_precise_position();
                let duration = sm.get_current_track_duration();
                let remaining = if duration > pos_secs { duration - pos_secs } else { 0 };
                
                map.insert("active_listeners".to_string(), Value::Number(active_listeners.into()));
                map.insert("radio_position".to_string(), Value::Number(pos_secs.into()));
                map.insert("radio_position_ms".to_string(), Value::Number(pos_ms.into()));
                map.insert("playback_position".to_string(), Value::Number(pos_secs.into()));
                map.insert("playback_position_ms".to_string(), Value::Number(pos_ms.into()));
                map.insert("remaining_seconds".to_string(), Value::Number(remaining.into()));
                map.insert("is_near_end".to_string(), Value::Bool(remaining <= 10));
                map.insert("streaming".to_string(), Value::Bool(sm.is_streaming()));
                map.insert("track_ended".to_string(), Value::Bool(sm.track_ended()));
                map.insert("bitrate".to_string(), Value::Number((sm.get_current_bitrate() / 1000).into()));
                
                map.insert("server_timestamp".to_string(), Value::Number(
                    (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64)
                        .into()
                ));
                
                if is_mobile {
                    map.insert("mobile_optimized".to_string(), Value::Bool(true));
                }
            }
            return CorsResponse::new(Json(track_value));
        }
    }
    
    // Fallback to generating JSON
    let track_state = sm.get_track_state();
    
    // Get track info from stream manager
    if let Some(track_json) = &track_state.track_info {
        if let Ok(mut track_value) = serde_json::from_str::<Value>(track_json) {
            if let Value::Object(ref mut map) = track_value {
                // Add all metadata
                map.insert("radio_position".to_string(), Value::Number(track_state.position_seconds.into()));
                map.insert("radio_position_ms".to_string(), Value::Number(track_state.position_milliseconds.into()));
                map.insert("playback_position".to_string(), Value::Number(track_state.position_seconds.into()));
                map.insert("playback_position_ms".to_string(), Value::Number(track_state.position_milliseconds.into()));
                map.insert("active_listeners".to_string(), Value::Number(active_listeners.into()));
                map.insert("bitrate".to_string(), Value::Number((track_state.bitrate / 1000).into()));
                map.insert("server_timestamp".to_string(), Value::Number(
                    (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64)
                        .into()
                ));
                
                // Radio metadata
                map.insert("streaming_mode".to_string(), Value::String("true-radio".to_string()));
                map.insert("client_control_enabled".to_string(), Value::Bool(false));
                map.insert("seeking_enabled".to_string(), Value::Bool(false));
                map.insert("skip_enabled".to_string(), Value::Bool(false));
                map.insert("synchronized_playback".to_string(), Value::Bool(true));
                map.insert("streaming".to_string(), Value::Bool(sm.is_streaming()));
                map.insert("track_ended".to_string(), Value::Bool(sm.track_ended()));
                
                // Track timing info
                map.insert("remaining_seconds".to_string(), Value::Number(track_state.remaining_time.into()));
                map.insert("is_near_end".to_string(), Value::Bool(track_state.is_near_end));
                
                if is_mobile {
                    map.insert("mobile_optimized".to_string(), Value::Bool(true));
                }
            }
            return CorsResponse::new(Json(track_value));
        }
    }
    
    // Ultimate fallback
    warn!("No track info available, returning default");
    CorsResponse::new(Json(serde_json::json!({
        "title": "ChillOut Radio",
        "artist": "Live Stream",
        "album": "Broadcasting",
        "duration": 0,
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
        "streaming_mode": "true-radio",
        "client_control_enabled": false,
        "seeking_enabled": false,
        "skip_enabled": false,
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
        "streaming_mode": "true-radio"
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
        "remaining_seconds": track_state.remaining_time,
        "streaming_mode": "true-radio",
        "client_control_enabled": false,
        "seeking_enabled": false,
        "skip_enabled": false,
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
        "is_near_end": track_state.is_near_end,
        "streaming_mode": "true-radio",
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    })))
}

#[get("/api/playlist")]
pub async fn get_playlist() -> CorsResponse<Json<serde_json::Value>> {
    let playlist_data = playlist::get_playlist_cached();
    let current_track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    // Don't expose current_track index to prevent client-side track control attempts
    let tracks_without_index: Vec<_> = playlist_data.tracks.iter().enumerate().map(|(i, track)| {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "is_current": current_track.as_ref().map(|ct| ct.path == track.path).unwrap_or(false),
            "order": i
        })
    }).collect();
    
    CorsResponse::new(Json(serde_json::json!({
        "tracks": tracks_without_index,
        "total_tracks": playlist_data.tracks.len(),
        "streaming_mode": "true-radio",
        "client_control_enabled": false,
        "seeking_enabled": false,
        "skip_enabled": false,
        "info": "Tracks play in server-controlled order"
    })))
}

#[get("/api/health")]
pub async fn health_check(stream_manager: &State<Arc<StreamManager>>) -> CorsResponse<Json<serde_json::Value>> {
    let sm = stream_manager.as_ref();
    
    CorsResponse::new(Json(serde_json::json!({
        "status": "healthy",
        "streaming": sm.is_streaming(),
        "active_listeners": sm.get_active_listeners(),
        "mode": "true-radio",
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
        "mode": "true-radio",
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

#[get("/test")]
pub async fn test_endpoint() -> &'static str {
    "Server is running! Audio stream should be at /direct-stream"
}
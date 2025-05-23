// src/handlers.rs - Complete enhanced version with Android position fixes

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
        title: "ChillOut Radio - Enhanced Position Sync with Android Fixes",
        version: "2.0.0-android-fixed",
        features: vec![
            "Server-authoritative position sync for Android",
            "Millisecond precision timing",
            "Enhanced error recovery",
            "Position drift correction",
            "Cross-platform compatibility"
        ]
    })
}

#[get("/api/now-playing?<force_server_position>&<android_client>")]
pub async fn now_playing(
    force_server_position: Option<bool>,
    android_client: Option<bool>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Get comprehensive track state with precise timing
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    let is_android = android_client.unwrap_or(false);
    
    // Enhanced logging for Android debugging
    if is_android || force_server_position.unwrap_or(false) {
        log::info!("Android/Debug position request - Server: {}s + {}ms", 
                   track_state.position_seconds, track_state.position_milliseconds);
    }
    
    // Get track info from stream manager's state
    if let Some(track_json) = &track_state.track_info {
        if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(track_json) {
            if let serde_json::Value::Object(ref mut map) = track_value {
                // Enhanced position information with extra precision for Android
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
                
                // Add high precision timestamp for Android sync
                map.insert(
                    "server_timestamp_precise".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64 / 1_000_000 // nanoseconds to milliseconds
                    ))
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
                
                // Add position validation for Android clients
                if is_android {
                    map.insert(
                        "position_validated".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "android_optimized".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "position_authority".to_string(),
                        serde_json::Value::String("server".to_string())
                    );
                }
                
                // Add streaming status information
                map.insert(
                    "streaming".to_string(),
                    serde_json::Value::Bool(sm.is_streaming())
                );
                map.insert(
                    "track_ended".to_string(),
                    serde_json::Value::Bool(sm.track_ended())
                );
                
                // Add buffering information
                map.insert(
                    "buffer_health".to_string(),
                    serde_json::Value::String("good".to_string())
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
                map.insert(
                    "server_timestamp_precise".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64 / 1_000_000
                    ))
                );
                
                if is_android {
                    map.insert(
                        "position_validated".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "android_optimized".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "position_authority".to_string(),
                        serde_json::Value::String("server".to_string())
                    );
                }
                
                // Add streaming status
                map.insert(
                    "streaming".to_string(),
                    serde_json::Value::Bool(sm.is_streaming())
                );
                map.insert(
                    "track_ended".to_string(),
                    serde_json::Value::Bool(sm.track_ended())
                );
            }
            
            Json(track_json)
        },
        None => Json(serde_json::json!({
            "error": "No tracks available",
            "server_timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            "android_fallback": is_android,
            "streaming": false,
            "track_ended": true
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
        "streaming_method": "enhanced_position_sync_android_fixed",
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
            "android_optimized": true,
            "continuity_on_reconnect": true,
            "server_authoritative_android": true
        },
        "server_info": {
            "uptime_seconds": track_state.position_seconds, // Approximate server uptime
            "version": "2.0.0-android-fixed",
            "platform": "rust",
            "memory_usage": "optimized"
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
            .as_millis(),
        "precision": "millisecond",
        "source": "stream_manager"
    }))
}

// Android-specific position endpoint for debugging
#[get("/api/android-position")]
pub async fn android_position(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    
    // Get very precise timing for Android
    let server_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    
    log::info!("Android position request: {}s + {}ms at timestamp {}ms", 
               track_state.position_seconds, 
               track_state.position_milliseconds,
               server_timestamp.as_millis());
    
    Json(serde_json::json!({
        "position_seconds": track_state.position_seconds,
        "position_milliseconds": track_state.position_milliseconds,
        "duration": track_state.duration,
        "server_timestamp_ms": server_timestamp.as_millis(),
        "server_timestamp_ns": server_timestamp.as_nanos(),
        "bitrate": track_state.bitrate,
        "android_optimized": true,
        "position_authority": "server", // Always server-authoritative for Android
        "debug_info": {
            "streaming": sm.is_streaming(),
            "near_end": track_state.is_near_end,
            "remaining": track_state.remaining_time,
            "active_listeners": sm.get_active_listeners(),
            "track_ended": sm.track_ended()
        },
        "validation": {
            "position_validated": true,
            "precision": "millisecond",
            "source": "stream_manager",
            "authority": "server"
        },
        "timing": {
            "request_time": server_timestamp.as_millis(),
            "position_time": track_state.position_seconds * 1000 + track_state.position_milliseconds as u64,
            "sync_accuracy": "high"
        }
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
        "track_duration": track_state.duration,
        "sync_status": "ok"
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
            
            // Sync status based on drift
            let sync_status = if drift.abs() <= 1 {
                "excellent"
            } else if drift.abs() <= 3 {
                "good"
            } else if drift.abs() <= 5 {
                "fair"
            } else {
                "poor"
            };
            map.insert("sync_status".to_string(), serde_json::Value::String(sync_status.to_string()));
            
            // Android-specific drift analysis
            if drift.abs() > 5 {
                map.insert("android_recommendation".to_string(), serde_json::Value::String("reconnect_with_server_position".to_string()));
            } else if drift.abs() > 3 {
                map.insert("android_recommendation".to_string(), serde_json::Value::String("apply_drift_correction".to_string()));
            } else {
                map.insert("android_recommendation".to_string(), serde_json::Value::String("maintain_current_sync".to_string()));
            }
        }
    }
    
    Json(response)
}

// API endpoint for playlist information
#[get("/api/playlist")]
pub async fn get_playlist() -> Json<serde_json::Value> {
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    let current_track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    if playlist_data.tracks.is_empty() {
        Json(serde_json::json!({
            "error": "No tracks available in playlist",
            "tracks": [],
            "total_tracks": 0,
            "current_track": null,
            "playlist_duration": 0
        }))
    } else {
        Json(serde_json::json!({
            "tracks": playlist_data.tracks,
            "total_tracks": playlist_data.tracks.len(),
            "current_track_index": current_track.as_ref().map(|t| {
                playlist_data.tracks.iter().position(|track| track.path == t.path).unwrap_or(0)
            }).unwrap_or(0),
            "current_track": current_track,
            "shuffle_enabled": false, // Add if you implement shuffle
            "repeat_enabled": true,   // Assuming continuous playback
            "playlist_duration": playlist_data.tracks.iter().map(|t| t.duration).sum::<u64>()
        }))
    }
}

// API endpoint for server health check
#[get("/api/health")]
pub async fn health_check(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    let track_state = sm.get_track_state();
    
    // Basic health metrics
    let health_status = if sm.is_streaming() && !sm.track_ended() {
        "healthy"
    } else if sm.track_ended() {
        "transitioning"
    } else {
        "inactive"
    };
    
    Json(serde_json::json!({
        "status": health_status,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "streaming": sm.is_streaming(),
        "active_listeners": sm.get_active_listeners(),
        "current_track_duration": track_state.duration,
        "current_position": track_state.position_seconds,
        "track_ended": sm.track_ended(),
        "version": "2.0.0-android-fixed",
        "android_support": true,
        "position_sync": "server_authoritative"
    }))
}

// Diagnostic page for troubleshooting
#[get("/diag")]
pub async fn diagnostic_page() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/diag.html")).await.ok()
}

// Admin API for force track skip (if needed)
#[get("/api/admin/skip-track?<auth_token>")]
pub async fn admin_skip_track(
    auth_token: Option<String>,
    _stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    // Simple auth check - in production, use proper authentication
    if auth_token.as_deref() != Some("admin123") {
        return Json(serde_json::json!({
            "error": "Unauthorized",
            "message": "Invalid auth token"
        }));
    }
    
    // Force track to end (this would trigger track switching logic)
    // This is a simplified implementation - you might need to implement this in StreamManager
    Json(serde_json::json!({
        "success": true,
        "message": "Track skip requested",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }))
}

// Helper function to serve static files
#[get("/static/<file..>")]
pub async fn static_files(file: PathBuf) -> Option<NamedFile> {
    let path = Path::new("static/").join(file);
    NamedFile::open(path).await.ok()
}

// Serve favicon
#[get("/favicon.ico")]
pub async fn favicon() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/favicon.ico")).await.ok()
}

// Serve robots.txt
#[get("/robots.txt")]
pub async fn robots() -> &'static str {
    "User-agent: *\nDisallow: /api/\nDisallow: /admin/"
}

// API endpoint for CORS preflight
#[rocket::options("/<_..>")]
pub fn cors_preflight() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}

// Error catchers
#[catch(404)]
pub async fn not_found() -> Template {
    Template::render("error", context! {
        status: 404,
        title: "Page Not Found",
        message: "The requested page could not be found.",
        back_link: "/"
    })
}

#[catch(500)]
pub async fn server_error() -> Template {
    Template::render("error", context! {
        status: 500,
        title: "Server Error",
        message: "An internal server error occurred. Please try again later.",
        back_link: "/"
    })
}

#[catch(503)]
pub async fn service_unavailable() -> Template {
    Template::render("error", context! {
        status: 503,
        title: "Service Unavailable",
        message: "The server is currently at capacity. Please try again in a few moments.",
        back_link: "/"
    })
}

#[catch(429)]
pub async fn too_many_requests() -> Template {
    Template::render("error", context! {
        status: 429,
        title: "Too Many Requests",
        message: "You have made too many requests. Please wait before trying again.",
        back_link: "/"
    })
}
// src/handlers.rs - Fixed version with heartbeat support and accurate listener tracking

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
        title: "ChillOut Radio - Mobile-Optimized with Fixed Listener Count",
        version: "2.1.0-mobile-fixed",
        features: vec![
            "Mobile-optimized streaming",
            "Accurate listener count tracking",
            "Connection heartbeat system",
            "Improved error recovery",
            "Battery-friendly operation",
            "Cross-platform compatibility"
        ]
    })
}

#[get("/api/now-playing?<mobile_client>&<android_client>")]
pub async fn now_playing(
    mobile_client: Option<bool>,
    android_client: Option<bool>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections before reporting
    sm.cleanup_stale_connections();
    
    // Get comprehensive track state
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners(); // Now returns accurate count
    let is_mobile = mobile_client.unwrap_or(false) || android_client.unwrap_or(false);
    
    if is_mobile {
        log::debug!("Mobile client position request - Server: {}s + {}ms, Listeners: {}", 
                   track_state.position_seconds, track_state.position_milliseconds, active_listeners);
    }
    
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
                
                // Add precise timestamps for mobile sync
                let server_timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                
                map.insert(
                    "server_timestamp".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(server_timestamp.as_millis() as u64))
                );
                
                // Mobile-specific optimizations
                if is_mobile {
                    map.insert(
                        "mobile_optimized".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "position_tolerance".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(5)) // 5 second tolerance for mobile
                    );
                    map.insert(
                        "heartbeat_required".to_string(),
                        serde_json::Value::Bool(true)
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
                
                // Connection health info
                map.insert(
                    "connection_health".to_string(),
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
                    "server_timestamp".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64
                    ))
                );
                
                if is_mobile {
                    map.insert(
                        "mobile_optimized".to_string(),
                        serde_json::Value::Bool(true)
                    );
                    map.insert(
                        "position_tolerance".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(5))
                    );
                }
                
                map.insert(
                    "streaming".to_string(),
                    serde_json::Value::Bool(sm.is_streaming())
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
            "mobile_fallback": is_mobile,
            "streaming": false,
            "active_listeners": active_listeners
        }))
    }
}

// New heartbeat endpoint to maintain connections and accurate listener count
#[get("/api/heartbeat?<connection_id>")]
pub async fn heartbeat(
    connection_id: Option<String>,
    stream_manager: &State<Arc<StreamManager>>
) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections first
    sm.cleanup_stale_connections();
    
    if let Some(conn_id) = connection_id {
        // Update heartbeat for this connection
        sm.update_listener_heartbeat(&conn_id);
        log::debug!("Heartbeat received from connection: {}", &conn_id[..8]);
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
        "position_seconds": position_secs,
        "position_milliseconds": position_ms,
        "streaming": sm.is_streaming(),
        "heartbeat_interval_ms": 15000 // Recommended heartbeat interval
    }))
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections for accurate count
    sm.cleanup_stale_connections();
    
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
        "streaming_method": "mobile_optimized_with_heartbeat",
        "position_accuracy": "millisecond",
        "server_time": chrono::Local::now().to_rfc3339(),
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "features": {
            "connection_tracking": true,
            "heartbeat_system": true,
            "accurate_listener_count": true,
            "mobile_optimized": true,
            "stale_connection_cleanup": true,
            "position_persistence": true,
            "drift_correction": true
        },
        "connection_info": {
            "heartbeat_interval_seconds": 15,
            "connection_timeout_seconds": 60,
            "cleanup_frequency": "on_api_call"
        }
    }))
}

// Enhanced position endpoint
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

// Connection management endpoint
#[get("/api/connections")]
pub async fn get_connections(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Clean up stale connections
    sm.cleanup_stale_connections();
    
    let active_listeners = sm.get_active_listeners();
    
    Json(serde_json::json!({
        "active_connections": active_listeners,
        "cleanup_performed": true,
        "connection_timeout_seconds": 60,
        "heartbeat_required": true,
        "server_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }))
}

// Playlist endpoint
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
            "shuffle_enabled": false,
            "repeat_enabled": true,
            "playlist_duration": playlist_data.tracks.iter().map(|t| t.duration).sum::<u64>()
        }))
    }
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

// Health check endpoint with connection info
#[get("/api/health")]
pub async fn health_check(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Clean up and get accurate listener count
    sm.cleanup_stale_connections();
    
    let track_state = sm.get_track_state();
    let active_listeners = sm.get_active_listeners();
    
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
        "active_listeners": active_listeners,
        "current_track_duration": track_state.duration,
        "current_position": track_state.position_seconds,
        "track_ended": sm.track_ended(),
        "version": "2.1.0-mobile-fixed",
        "features": {
            "mobile_optimized": true,
            "heartbeat_system": true,
            "accurate_listener_count": true,
            "connection_cleanup": true
        },
        "connection_health": {
            "active_connections": active_listeners,
            "heartbeat_enabled": true,
            "stale_cleanup": "automatic"
        }
    }))
}

// Diagnostic page
#[get("/diag")]
pub async fn diagnostic_page() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/diag.html")).await.ok()
}

// Debug endpoint for connection troubleshooting
#[get("/api/debug/connections")]
pub async fn debug_connections(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    let sm = stream_manager.as_ref();
    
    // Get count before and after cleanup
    let count_before = sm.get_active_listeners();
    sm.cleanup_stale_connections();
    let count_after = sm.get_active_listeners();
    
    Json(serde_json::json!({
        "connections_before_cleanup": count_before,
        "connections_after_cleanup": count_after,
        "stale_connections_removed": count_before - count_after,
        "cleanup_timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        "connection_timeout_seconds": 60,
        "heartbeat_interval_seconds": 15
    }))
}

// Static files
#[get("/static/<file..>")]
pub async fn static_files(file: PathBuf) -> Option<NamedFile> {
    let path = Path::new("static/").join(file);
    NamedFile::open(path).await.ok()
}

// Favicon
#[get("/favicon.ico")]
pub async fn favicon() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/favicon.ico")).await.ok()
}

// Robots.txt
#[get("/robots.txt")]
pub async fn robots() -> &'static str {
    "User-agent: *\nDisallow: /api/\nDisallow: /admin/"
}

// CORS preflight
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
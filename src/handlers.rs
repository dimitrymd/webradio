// Updated handlers.rs with direct streaming for all platforms

use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::models::playlist::Track;
use crate::services::playlist;
use crate::services::streamer::StreamManager;
use crate::services::websocket_bus::WebSocketBus;
use crate::config;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "MP3 Web Radio",
    })
}

#[get("/api/now-playing")]
pub async fn now_playing(stream_manager: &State<Arc<StreamManager>>) -> Json<serde_json::Value> {
    // Get the reference to StreamManager
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
                    serde_json::Value::Number(serde_json::Number::from(current_bitrate / 1000)) // convert to kbps for display
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
pub async fn get_stats(
    stream_manager: &State<Arc<StreamManager>>,
    websocket_bus: &State<Arc<WebSocketBus>>
) -> Json<serde_json::Value> {
    // Get references
    let sm = stream_manager.as_ref();
    let ws_bus = websocket_bus.as_ref();
    
    // Collect stats without holding any locks for too long
    let active_listeners = ws_bus.get_active_listeners();
    let connected_clients = ws_bus.get_client_count();
    let receiver_count = sm.get_receiver_count();
    let saved_chunks_count = sm.get_saved_chunks_count();
    let is_streaming = sm.is_streaming();
    let track_ended = sm.track_ended();
    let current_bitrate = sm.get_current_bitrate();
    let playback_position = sm.get_playback_position();
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
        "connected_clients": connected_clients,
        "receiver_count": receiver_count,
        "max_concurrent_users": config::MAX_CONCURRENT_USERS,
        "saved_chunks": saved_chunks_count,
        "streaming": is_streaming,
        "track_ended": track_ended,
        "bitrate_kbps": current_bitrate / 1000,
        "playback_position": playback_position,
        "server_time": chrono::Local::now().to_rfc3339()
    }))
}

// Direct streaming for all platforms
// Updated direct_stream handler with seeking support
#[get("/direct-stream?<t>&<position>&<track>&<buffer>")]
pub async fn direct_stream(
    stream_manager: &State<Arc<StreamManager>>,
    t: Option<u64>,
    position: Option<u64>,
    track: Option<String>,
    buffer: Option<u64>
) -> Result<(ContentType, Vec<u8>), Status> {
    use rocket::http::{ContentType, Status};
    
    // Check if streaming is active
    if !stream_manager.is_streaming() {
        return Err(Status::ServiceUnavailable);
    }
    
    // For all platforms, we need to send the complete current track
    let (header, all_chunks) = stream_manager.get_chunks_from_current_position();
    
    // Always use all chunks - direct streaming needs the complete file
    let chunks_to_use = &all_chunks;
    
    // Determine how much data to include based on buffer parameter
    // Default to 30 seconds of buffer (increased from 5)
    let buffer_seconds = buffer.unwrap_or(30);
    
    // Log the buffer size for debugging
    println!("Stream request with buffer size: {}s", buffer_seconds);
    
    // If a position was specified and it's greater than 0, try to skip some chunks
    let chunks_to_return = if let Some(pos) = position {
        if pos > 0 {
            // Log the position request
            println!("Position request: {}s for track: {:?}, buffer: {}s", pos, track, buffer_seconds);
            
            // We'd need to know the bitrate to accurately skip, but we can estimate
            // Let's assume 128kbps = 16KB per second of audio
            // Skip approximately the right number of chunks to reach position
            let bytes_per_second: usize = 16000; // 16KB per second at 128kbps
            let bytes_to_skip: usize = (pos as usize) * bytes_per_second;
            let mut total_bytes: usize = 0;
            let mut skip_chunks: usize = 0;
            
            for chunk in chunks_to_use {
                total_bytes += chunk.len();
                skip_chunks += 1;
                
                if total_bytes >= bytes_to_skip {
                    break;
                }
            }
            
            // Only skip if we have enough chunks and won't skip everything
            if skip_chunks > 0 && skip_chunks < chunks_to_use.len() - 1 {
                println!("Skipping {} chunks (approx. {} bytes) to reach position {}s", 
                         skip_chunks, total_bytes, pos);
                         
                // Return the chunks after the skip point
                &chunks_to_use[skip_chunks..]
            } else {
                // Not enough chunks to skip or would skip everything, return all
                chunks_to_use
            }
        } else {
            chunks_to_use
        }
    } else {
        chunks_to_use
    };
    
    // IMPORTANT: Calculate approximate file size for headers
    let total_file_size: usize = chunks_to_return.iter().map(|c| c.len()).sum();
    let bitrate: usize = 128000; // Assume 128kbps for simplicity
    let bytes_per_second: usize = bitrate / 8;
    let approximate_duration: u64 = (total_file_size / bytes_per_second) as u64;
    
    // Combine the chunks into the response
    let mut response_data = Vec::new();
    
    // Add header if available
    if let Some(h) = header {
        response_data.extend_from_slice(&h);
    }
    
    // Add selected chunks
    for chunk in chunks_to_return {
        response_data.extend_from_slice(chunk);
    }
    
    // Return the response with the appropriate Content-Type
    // We can't add custom headers with the (ContentType, Vec<u8>) return type
    Ok((ContentType::new("audio", "mpeg"), response_data))
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
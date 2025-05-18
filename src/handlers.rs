// src/handlers.rs - Updated with better position handling and track id tracking

use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

#[get("/direct-stream?<t>&<position>&<track>&<buffer>&<slow>")]
pub async fn direct_stream(
    stream_manager: &State<Arc<StreamManager>>,
    t: Option<u64>,
    position: Option<u64>,
    track: Option<String>,
    buffer: Option<u64>,
    slow: Option<bool>
) -> Result<(ContentType, Vec<u8>), Status> {
    // Check if streaming is active
    if !stream_manager.is_streaming() {
        return Err(Status::ServiceUnavailable);
    }
    
    // Get the current server position and track
    let server_position = stream_manager.get_playback_position();
    let current_track_id = stream_manager.get_current_track_id();
    
    // Log request details
    println!("Stream request: position={:?}, track={:?}, buffer={:?}, slow={:?} (server at {}s)", 
             position, track, buffer, slow, server_position);
    
    // Check if requested track matches current track
    let track_match = if let Some(ref req_track) = track {
        if let Some(ref cur_track) = current_track_id {
            req_track == cur_track
        } else {
            true // No current track ID available, assume match
        }
    } else {
        true // No track specified, assume match
    };
    
    // Handle track mismatch - client might be requesting an old track
    if !track_match {
        println!("Track mismatch - client requested {:?} but server is playing {:?}", 
                 track, current_track_id);
    }
    
    // For all platforms, we need to send the current track chunks
    let (header, all_chunks) = stream_manager.get_chunks_from_current_position();
    
    // Always use all chunks - direct streaming needs the complete file
    let chunks_to_use = &all_chunks;
    
    // Determine how much data to include based on buffer parameter
    // Default to 60 seconds of buffer - increased for better stability
    let buffer_seconds = buffer.unwrap_or(60);
    
    // Use the requested position or server position
    // If there's a track mismatch, always use server position
    let effective_position = if track_match {
        position.unwrap_or(server_position)
    } else {
        // Track mismatch - use server position
        println!("Using server position due to track mismatch");
        server_position
    };
    
    // Handle slow network flag
    let chunk_size_modifier = if slow.unwrap_or(false) {
        println!("Using smaller chunk sizes for slow network");
        0.5 // Use half-size chunks
    } else {
        1.0 // Normal size
    };
    
    println!("Effective position: {}s (server at {}s)", effective_position, server_position);
    
    // If position specified and > 0, try to skip some chunks
    let chunks_to_return = if effective_position > 0 {
        // Get current bitrate for better accuracy
        let bitrate = stream_manager.get_current_bitrate();
        let bytes_per_second = (bitrate / 8) as usize;
        
        // Check if we have enough chunks
        if chunks_to_use.len() < 10 {
            println!("Not enough chunks available ({}), sending all", chunks_to_use.len());
            return Ok((ContentType::new("audio", "mpeg"), 
                     combine_chunks(header.as_ref(), chunks_to_use)));
        }
        
        // Calculate bytes to skip based on position
        let bytes_to_skip: usize = effective_position as usize * bytes_per_second;
        let mut total_bytes: usize = 0;
        let mut skip_chunks: usize = 0;
        
        // Count chunks to skip
        for chunk in chunks_to_use {
            total_bytes += chunk.len();
            skip_chunks += 1;
            
            if total_bytes >= bytes_to_skip {
                break;
            }
        }
        
        // Safety check - ensure we don't skip too many chunks
        let max_skip = chunks_to_use.len().saturating_sub(10);
        if skip_chunks > max_skip {
            skip_chunks = max_skip;
            println!("Limiting skip to {} chunks to ensure enough data", skip_chunks);
        }
        
        if skip_chunks > 0 && skip_chunks < chunks_to_use.len() {
            println!("Skipping {} chunks to reach position {}s", 
                    skip_chunks, effective_position);
                    
            // Return the chunks after the skip point
            &chunks_to_use[skip_chunks..]
        } else {
            println!("Skip calculation resulted in skip_chunks={}, using all chunks", 
                    skip_chunks);
            chunks_to_use
        }
    } else {
        // No position or position=0, use all chunks
        chunks_to_use
    };
    
    // Return combined chunks with the appropriate Content-Type
    Ok((ContentType::new("audio", "mpeg"), 
       combine_chunks(header.as_ref(), chunks_to_return)))
}

// Helper function to combine header and chunks
fn combine_chunks(header: Option<&Vec<u8>>, chunks: &[Vec<u8>]) -> Vec<u8> {
    // Calculate total size
    let header_size = header.map_or(0, |h| h.len());
    let chunks_size: usize = chunks.iter().map(|c| c.len()).sum();
    let total_size = header_size + chunks_size;
    
    // Allocate with capacity
    let mut response_data = Vec::with_capacity(total_size);
    
    // Add header if available
    if let Some(h) = header {
        response_data.extend_from_slice(h);
    }
    
    // Add chunks
    for chunk in chunks {
        response_data.extend_from_slice(chunk);
    }
    
    response_data
}

#[get("/test")]
pub async fn test_page() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/test.html")).await.ok()
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
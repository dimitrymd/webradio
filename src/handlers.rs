// Fixed handlers.rs with borrowing issue resolved

use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use rocket_ws as ws;
use rocket::futures::SinkExt; // For WebSocket streaming
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use futures::stream::StreamExt;
use std::collections::VecDeque;

use crate::models::playlist::Playlist;
use crate::services::playlist;
use crate::services::streamer::StreamManager;
use crate::config;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "MP3 Web Radio",
    })
}

#[get("/api/now-playing")]
pub async fn now_playing(stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    // Get the inner reference to StreamManager
    let sm = stream_manager.inner();
    
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
pub async fn get_stats(stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    // Get inner reference to StreamManager
    let sm = stream_manager.inner();
    
    // Collect stats without holding any locks for too long
    let active_listeners = sm.get_active_listeners();
    let receiver_count = sm.get_receiver_count();
    let saved_chunks_count = sm.get_saved_chunks_count();
    let is_streaming = sm.is_streaming();
    let track_ended = sm.track_ended();
    let current_bitrate = sm.get_current_bitrate();
    let playback_position = sm.get_playback_position();
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
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

// Significantly improved WebSocket handler for streaming audio
// Fix: resolved borrowing issue in tokio::select! block
#[get("/stream")]
pub fn stream_ws(ws: ws::WebSocket, stream_manager: &State<StreamManager>) -> ws::Channel<'static> {
    let stream_manager_clone = stream_manager.inner().clone();
    
    ws.channel(move |stream| Box::pin(async move {
        // Check if streaming is active
        if !stream_manager_clone.is_streaming() {
            let mut stream = stream;
            let _ = stream.send(ws::Message::Text(serde_json::json!({
                "error": "Streaming is not currently active"
            }).to_string())).await;
            return Ok(());
        }
        
        // Get initial data
        let track_info = stream_manager_clone.get_track_info();
        let (id3_header, saved_chunks) = stream_manager_clone.get_chunks_from_current_position();
        
        // Create broadcast receiver
        let mut broadcast_rx = stream_manager_clone.get_broadcast_receiver();
        
        // Increment listener count
        stream_manager_clone.increment_listener_count();
        let listener_id = stream_manager_clone.get_active_listeners();
        println!("Listener {} connected", listener_id);
        
        // Wrap stream in Arc for sharing
        let stream = Arc::new(tokio::sync::Mutex::new(stream));
        
        // Send initial track info
        if let Some(info) = track_info {
            if let Err(e) = stream.lock().await.send(ws::Message::Text(info)).await {
                println!("Error sending track info to listener {}: {:?}", listener_id, e);
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
        }
        
        // Send ID3 header
        if let Some(id3) = id3_header {
            if let Err(e) = stream.lock().await.send(ws::Message::Binary(id3)).await {
                println!("Error sending ID3 header to listener {}: {:?}", listener_id, e);
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
            
            // Add a small delay after ID3 header to allow client processing
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Send more initial chunks to bootstrap playback - increased from original
        // Use the config parameter for initial chunk count
        let non_empty_chunks: Vec<Vec<u8>> = saved_chunks.into_iter()
            .filter(|chunk| !chunk.is_empty())
            .collect();
            
        let initial_chunks_count = std::cmp::min(non_empty_chunks.len(), config::INITIAL_CHUNKS_TO_SEND);
        println!("Sending {} initial chunks to listener {}", initial_chunks_count, listener_id);
        
        for chunk in non_empty_chunks.iter().take(initial_chunks_count) {
            match stream.lock().await.send(ws::Message::Binary(chunk.clone())).await {
                Ok(_) => {
                    // Small delay between initial chunks, but not too much (20ms -> 10ms)
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                },
                Err(e) => {
                    println!("Error sending initial chunks to listener {}: {:?}", listener_id, e);
                    stream_manager_clone.decrement_listener_count();
                    return Ok(());
                }
            }
        }
        
        // Set up ping timer
        let ping_interval = tokio::time::Duration::from_millis(config::WS_PING_INTERVAL_MS);
        let mut ping_timer = tokio::time::interval(ping_interval);
        
        // Main streaming loop - deliver audio data with improved error handling
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 5; // Increased from 3
        let mut last_activity = Instant::now();
        let mut chunk_counter = 0;
        
        // Fix: store a mutable reference to the stream to avoid temporary value error
        let mut stream_lock = stream.lock().await;
        
        loop {
            // Check for timeout
            if last_activity.elapsed() > Duration::from_secs(config::WS_TIMEOUT_SECS) {
                println!("Client {} timeout after {}s", listener_id, config::WS_TIMEOUT_SECS);
                break;
            }
            
            tokio::select! {
                // Handle ping timer
                _ = ping_timer.tick() => {
                    if let Err(e) = stream_lock.send(ws::Message::Ping(vec![])).await {
                        println!("Error sending ping to listener {}: {:?}", listener_id, e);
                        consecutive_errors += 1;
                        if consecutive_errors >= max_consecutive_errors {
                            break;
                        }
                    }
                }
                
                // Receive chunks from broadcast
                chunk_result = broadcast_rx.recv() => {
                    match chunk_result {
                        Ok(chunk) => {
                            // Process received chunk
                            last_activity = Instant::now();
                            chunk_counter += 1;
                            
                            if let Err(e) = stream_lock.send(ws::Message::Binary(chunk)).await {
                                println!("Error sending audio data to listener {}: {:?}", listener_id, e);
                                consecutive_errors += 1;
                                if consecutive_errors >= max_consecutive_errors {
                                    break;
                                }
                            } else {
                                consecutive_errors = 0; // Reset on successful send
                                
                                // Log progress occasionally
                                if chunk_counter % 500 == 0 {
                                    println!("Sent {} chunks to listener {}", chunk_counter, listener_id);
                                }
                            }
                        },
                        Err(e) => {
                            // Broadcast error
                            if e.to_string().contains("lagged") {
                                println!("Client {} broadcast lag, resubscribing", listener_id);
                                broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                            } else {
                                println!("Broadcast error for listener {}: {:?}", listener_id, e);
                                consecutive_errors += 1;
                                if consecutive_errors >= max_consecutive_errors {
                                    break;
                                }
                            }
                        }
                    }
                }
                
                // Process any messages from client - fix: use stream.clone() to avoid temporary borrow
                msg = stream_lock.next() => {
                    match msg {
                        Some(Ok(ws::Message::Close(_))) => {
                            println!("Client {} sent close", listener_id);
                            break;
                        },
                        Some(Ok(ws::Message::Pong(_))) => {
                            last_activity = Instant::now();
                        },
                        Some(Ok(ws::Message::Text(text))) => {
                            // Handle text messages (could be commands or feedback)
                            println!("Client {} sent message: {}", listener_id, text);
                            last_activity = Instant::now();
                        },
                        Some(Err(e)) => {
                            println!("Client {} message error: {:?}", listener_id, e);
                            consecutive_errors += 1;
                            if consecutive_errors >= max_consecutive_errors {
                                break;
                            }
                        },
                        _ => {}
                    }
                }
                
                // Add a timeout to keep the select! responsive
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                    // Just a timeout to prevent blocking - do nothing
                }
            }
        }
        
        // Release the lock explicitly before cleanup
        drop(stream_lock);
        
        // Cleanup
        stream_manager_clone.decrement_listener_count();
        println!("Client {} disconnected after receiving {} chunks", listener_id, chunk_counter);
        
        Ok(())
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
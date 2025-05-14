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
    
    Json(serde_json::json!({
        "active_listeners": active_listeners,
        "receiver_count": receiver_count,
        "max_concurrent_users": config::MAX_CONCURRENT_USERS,
        "saved_chunks": saved_chunks_count,
        "streaming": is_streaming,
        "track_ended": track_ended,
        "server_time": chrono::Local::now().to_rfc3339()
    }))
}

// WebSocket handler for streaming audio - Fixed version with live join support
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
            if let Err(_) = stream.lock().await.send(ws::Message::Text(info)).await {
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
        }
        
        // Send ID3 header
        if let Some(id3) = id3_header {
            if let Err(_) = stream.lock().await.send(ws::Message::Binary(id3)).await {
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
        }
        
        // Enhanced buffer management for smoother playback
        let initial_chunks = saved_chunks.len();
        let mut client_buffer_chunks = 0; // Track how many chunks we've sent to client

        // Calculate recommended buffer size based on latency
        // We want to ensure client has enough data to handle network jitter
        const MIN_CLIENT_BUFFER: usize = 20;  // Minimum chunks to buffer for smooth playback
        const MAX_CLIENT_BUFFER: usize = 200; // Maximum chunks to avoid excessive memory usage
        
        let mut chunk_queue: VecDeque<Vec<u8>> = VecDeque::from(saved_chunks);
        let mut sent_chunks = 0;
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 3;
        let mut last_activity = Instant::now();
        
        println!("Client {} starting with {} buffered chunks", listener_id, initial_chunks);
        
        // Measure initial latency to calculate optimal buffer size
        let latency_start = Instant::now();
        if let Err(_) = stream.lock().await.send(ws::Message::Ping(vec![1,2,3,4])).await {
            stream_manager_clone.decrement_listener_count();
            return Ok(());
        }
        
        // Dynamic buffer size based on conditions
        let mut target_buffer_size = MIN_CLIENT_BUFFER;
        
        // Main streaming loop with improved buffering
        loop {
            // Check for timeout
            if last_activity.elapsed() > Duration::from_secs(30) {
                println!("Client {} timeout", listener_id);
                break;
            }
            
            // Send buffered chunks first, respecting client-side buffer limits
            if !chunk_queue.is_empty() && client_buffer_chunks < target_buffer_size {
                let chunk = chunk_queue.pop_front().unwrap();
                last_activity = Instant::now();
                
                if !chunk.is_empty() {
                    match stream.lock().await.send(ws::Message::Binary(chunk)).await {
                        Ok(_) => {
                            sent_chunks += 1;
                            client_buffer_chunks += 1;
                            consecutive_errors = 0;
                        },
                        Err(_) => {
                            consecutive_errors += 1;
                            if consecutive_errors >= max_consecutive_errors {
                                println!("Client {} too many errors", listener_id);
                                break;
                            }
                        }
                    }
                }
                continue;
            }
            
            // If we have enough buffered on client, wait for more data or buffer to deplete
            if client_buffer_chunks >= target_buffer_size {
                // Receive new chunks with timeout - shorter timeout if we have a good buffer
                let timeout_duration = if client_buffer_chunks > MIN_CLIENT_BUFFER {
                    Duration::from_millis(100) // Quick check for more data
                } else {
                    Duration::from_millis(500) // Longer wait if buffer is low
                };
                
                match tokio::time::timeout(timeout_duration, broadcast_rx.recv()).await {
                    Ok(Ok(chunk)) => {
                        // Successful receive, reset activity timer
                        last_activity = Instant::now();
                        
                        // Check for special marker chunks
                        if chunk.len() == 2 {
                            let marker = (chunk[0], chunk[1]);
                            match marker {
                                (0xFF, 0xFE) => {
                                    // Track transition - clear buffers
                                    println!("Client {} track transition", listener_id);
                                    chunk_queue.clear();
                                    client_buffer_chunks = 0;
                                    
                                    // Get new track info
                                    if let Some(new_info) = stream_manager_clone.get_track_info() {
                                        if let Err(_) = stream.lock().await.send(ws::Message::Text(new_info)).await {
                                            break;
                                        }
                                    }
                                    
                                    // Get new track data
                                    let (new_id3, new_chunks) = stream_manager_clone.get_chunks_from_current_position();
                                    
                                    // Send new ID3
                                    if let Some(id3) = new_id3 {
                                        if let Err(_) = stream.lock().await.send(ws::Message::Binary(id3)).await {
                                            break;
                                        }
                                    }
                                    
                                    // Queue new chunks
                                    chunk_queue = VecDeque::from(new_chunks);
                                    continue;
                                },
                                (0xFF, 0xFF) => {
                                    // Track end
                                    continue;
                                },
                                _ => {}
                            }
                        }
                        
                        // Add regular chunk to queue
                        chunk_queue.push_back(chunk);
                    },
                    Ok(Err(e)) => {
                        // Broadcast error handling
                        if e.to_string().contains("lagged") {
                            println!("Client {} broadcast lag, resubscribing", listener_id);
                            broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                            
                            // Get fresh chunks to resync
                            let (_, fresh_chunks) = stream_manager_clone.get_chunks_from_current_position();
                            chunk_queue = VecDeque::from(fresh_chunks);
                            client_buffer_chunks = 0; // Reset buffer count
                            
                            // Adjust buffer size higher when we see lag
                            target_buffer_size = (target_buffer_size * 3 / 2).min(MAX_CLIENT_BUFFER);
                            println!("Client {} increasing buffer to {} chunks", listener_id, target_buffer_size);
                        } else {
                            consecutive_errors += 1;
                            if consecutive_errors >= max_consecutive_errors {
                                break;
                            }
                        }
                    },
                    Err(_) => {
                        // Timeout is normal if we're well buffered
                        // Estimate how many chunks client has consumed based on time
                        let time_since_last_chunk = last_activity.elapsed();
                        let consumed_chunks = (time_since_last_chunk.as_secs_f64() / 0.1).ceil() as usize; // Assume ~10 chunks per second
                        
                        if consumed_chunks > 0 && client_buffer_chunks > consumed_chunks {
                            client_buffer_chunks -= consumed_chunks;
                            // Gradually adjust buffer size based on connection stability
                            if consecutive_errors == 0 && sent_chunks > 500 {
                                // Connection seems stable, we can reduce buffer slightly
                                target_buffer_size = target_buffer_size.saturating_sub(1).max(MIN_CLIENT_BUFFER);
                            }
                        }
                        
                        // Send ping to check connection
                        if let Err(_) = stream.lock().await.send(ws::Message::Ping(vec![])).await {
                            break;
                        }
                        
                        if chunk_queue.is_empty() && client_buffer_chunks < MIN_CLIENT_BUFFER/2 {
                            // We're running low on buffer and not getting data
                            println!("Client {} buffer critically low: {} chunks", listener_id, client_buffer_chunks);
                            
                            // Try to get new chunks to refill buffer
                            let (_, fresh_chunks) = stream_manager_clone.get_chunks_from_current_position();
                            if !fresh_chunks.is_empty() {
                                println!("Client {} refilling with {} new chunks", listener_id, fresh_chunks.len());
                                chunk_queue = VecDeque::from(fresh_chunks);
                            }
                        }
                    }
                }
            }
        }
        
        // Cleanup
        stream_manager_clone.decrement_listener_count();
        println!("Client {} disconnected after {} chunks", listener_id, sent_chunks);
        
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
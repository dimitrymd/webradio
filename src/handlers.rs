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
    // Get the actual current track from the stream manager's state
    let track_info = stream_manager.get_track_info();
    let playback_position = stream_manager.get_playback_position();
    let active_listeners = stream_manager.get_active_listeners();
    
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
    // Collect stats without holding any locks for too long
    let active_listeners = stream_manager.get_active_listeners();
    let receiver_count = stream_manager.get_receiver_count();
    let saved_chunks_count = stream_manager.get_saved_chunks_count();
    let is_streaming = stream_manager.is_streaming();
    let track_ended = stream_manager.track_ended();
    
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
        
        // Buffer management
        let mut chunk_queue = VecDeque::from(saved_chunks);
        let initial_chunks = chunk_queue.len();
        let mut sent_chunks = 0;
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 3;
        let mut last_activity = Instant::now();
        
        println!("Client {} starting with {} buffered chunks", listener_id, initial_chunks);
        
        // Main streaming loop
        loop {
            // Check for timeout
            if last_activity.elapsed() > Duration::from_secs(30) {
                println!("Client {} timeout", listener_id);
                break;
            }
            
            // Send buffered chunks first
            if let Some(chunk) = chunk_queue.pop_front() {
                last_activity = Instant::now();
                
                if !chunk.is_empty() {
                    match stream.lock().await.send(ws::Message::Binary(chunk)).await {
                        Ok(_) => {
                            sent_chunks += 1;
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
            
            // Receive new chunks with timeout
            match tokio::time::timeout(Duration::from_secs(3), broadcast_rx.recv()).await {
                Ok(Ok(chunk)) => {
                    last_activity = Instant::now();
                    
                    // Handle special markers
                    if chunk.len() == 2 {
                        let marker = (chunk[0], chunk[1]);
                        match marker {
                            (0xFF, 0xFE) => {
                                // Track transition
                                println!("Client {} received track transition", listener_id);
                                
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
                                println!("Client {} queued {} chunks for new track", 
                                       listener_id, chunk_queue.len());
                                continue;
                            },
                            (0xFF, 0xFF) => {
                                // Track end
                                continue;
                            },
                            _ => {}
                        }
                    }
                    
                    // Send regular chunk
                    if !chunk.is_empty() {
                        match stream.lock().await.send(ws::Message::Binary(chunk)).await {
                            Ok(_) => {
                                sent_chunks += 1;
                                consecutive_errors = 0;
                                
                                if sent_chunks % 200 == 0 {
                                    println!("Client {} sent {} chunks", listener_id, sent_chunks);
                                }
                            },
                            Err(_) => {
                                consecutive_errors += 1;
                                if consecutive_errors >= max_consecutive_errors {
                                    break;
                                }
                            }
                        }
                    }
                },
                Ok(Err(e)) => {
                    if e.to_string().contains("lagged") {
                        println!("Client {} lagged, resubscribing", listener_id);
                        broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                        
                        // Get fresh chunks
                        let (_, fresh_chunks) = stream_manager_clone.get_chunks_from_current_position();
                        chunk_queue = VecDeque::from(fresh_chunks);
                    } else {
                        consecutive_errors += 1;
                        if consecutive_errors >= max_consecutive_errors {
                            break;
                        }
                    }
                },
                Err(_) => {
                    // Timeout is normal during streaming
                    // Send ping to check connection
                    if let Err(_) = stream.lock().await.send(ws::Message::Ping(vec![])).await {
                        break;
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
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
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    match track {
        Some(track) => {
            // Add active listener count
            let mut track_json = serde_json::to_value(track).unwrap_or_default();
            if let serde_json::Value::Object(ref mut map) = track_json {
                map.insert(
                    "active_listeners".to_string(), 
                    serde_json::Value::Number(serde_json::Number::from(stream_manager.get_active_listeners()))
                );
                
                // Add playback position
                map.insert(
                    "playback_position".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(stream_manager.get_playback_position()))
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

// WebSocket handler for streaming audio - NON-BLOCKING VERSION
#[get("/stream")]
pub fn stream_ws(ws: ws::WebSocket, stream_manager: &State<StreamManager>) -> ws::Channel<'static> {
    println!("WebSocket connection request received");
    
    // Check if max concurrent users limit is reached - do this quickly without locks
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        // Return an error message and close the connection
        println!("Max concurrent users limit reached");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("Server at capacity, try again later".into())).await;
            Ok(())
        }));
    }
    
    // Quick check for tracks availability without holding locks
    if playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER).is_none() {
        // Return an error message and close the connection
        println!("No tracks available");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("No tracks available".into())).await;
            Ok(())
        }));
    }
    
    // Clone the stream manager for use in the closure
    let stream_manager_clone = stream_manager.inner().clone();
    
    // Create a WebSocket channel
    ws.channel(move |stream| Box::pin(async move {
        // Use an Arc to share the stream between the heartbeat task and main task
        let stream = Arc::new(tokio::sync::Mutex::new(stream));
        
        // Prepare broadcast subscription BEFORE incrementing listeners
        // to ensure we don't miss messages
        let mut broadcast_rx = stream_manager_clone.get_broadcast_receiver();
        
        // Increment active listener count - must be done before setup
        // to ensure proper state tracking
        stream_manager_clone.increment_listener_count();
        let listener_id = stream_manager_clone.get_active_listeners();
        println!("Listener {} connected. Active listeners: {}", listener_id, stream_manager_clone.get_active_listeners());
        
        // Keep track of client state
        let client_connected = Arc::new(AtomicBool::new(true));
        let client_connected_clone = client_connected.clone();
        let stream_clone = stream.clone();
        
        // Set up heartbeat task to detect disconnected clients
        let heartbeat_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            while client_connected_clone.load(Ordering::Relaxed) {
                interval.tick().await;
                // Check if client is still connected
                if let Err(_) = stream_clone.lock().await.send(ws::Message::Ping(vec![1, 2, 3])).await {
                    // Client disconnected
                    client_connected_clone.store(false, Ordering::Relaxed);
                    break;
                }
            }
        });

        // Send track info first - QUICK OPERATION
        if let Some(track_info) = stream_manager_clone.get_track_info() {
            println!("Sending track info to client {}: {}", listener_id, track_info);
            if let Err(e) = stream.lock().await.send(ws::Message::Text(track_info)).await {
                println!("Error sending track info to client {}: {:?}", listener_id, e);
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
        }
        
        // Get saved data from the current playback position (instead of from the beginning)
        let (id3_header_opt, saved_chunks) = stream_manager_clone.get_chunks_from_current_position();
        
        // Send ID3 header first - QUICK OPERATION
        if let Some(id3_header) = id3_header_opt {
            println!("Sending ID3 header to client {} ({} bytes)", listener_id, id3_header.len());
            if let Err(e) = stream.lock().await.send(ws::Message::Binary(id3_header)).await {
                println!("Error sending ID3 header to client {}: {:?}", listener_id, e);
                stream_manager_clone.decrement_listener_count();
                return Ok(());
            }
        }
        
        // Send saved chunks in a separate async task to avoid blocking
        // This allows the main task to continue receiving broadcast messages
        // immediately, even while chunks are being sent
        let saved_chunks_count = saved_chunks.len();
        
        // Only spawn separate task if there are chunks to send
        if saved_chunks_count > 0 {
            println!("Sending {} saved chunks to client {} for catch-up", saved_chunks_count, listener_id);
            
            // Create a clone for the task
            let stream_for_chunks = stream.clone();
            let client_connected_for_chunks = client_connected.clone();
            let listener_id_copy = listener_id;
            
            // Spawn a separate task to send the chunks
            tokio::spawn(async move {
                // Track progress for debugging
                let mut chunks_sent = 0;
                let start_time = Instant::now();
                
                // Send chunks with rate limiting to avoid overwhelming the client
                for (i, chunk) in saved_chunks.iter().enumerate() {
                    // Check if client is still connected
                    if !client_connected_for_chunks.load(Ordering::Relaxed) {
                        println!("Client {} disconnected during chunk catch-up after sending {}/{} chunks",
                                listener_id_copy, chunks_sent, saved_chunks_count);
                        break;
                    }
                    
                    if !chunk.is_empty() {
                        match stream_for_chunks.lock().await.send(ws::Message::Binary(chunk.clone())).await {
                            Ok(_) => {
                                chunks_sent += 1;
                                // Log progress periodically
                                if chunks_sent % 100 == 0 {
                                    println!("Sent {}/{} catch-up chunks to client {}", 
                                           chunks_sent, saved_chunks_count, listener_id_copy);
                                }
                            },
                            Err(e) => {
                                println!("Error sending saved chunk {} to client {}: {:?}", 
                                       i, listener_id_copy, e);
                                break;
                            }
                        }
                        
                        // Rate limit the sending based on size 
                        // Larger chunks = more delay to avoid overwhelming the client
                        let delay_ms = if chunk.len() > 32768 {
                            20 // Larger chunk = more delay
                        } else if chunk.len() > 16384 {
                            10
                        } else {
                            5 // Small chunk = minimal delay
                        };
                        
                        // Only apply delay every few chunks to maintain throughput
                        if i % 10 == 0 {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }
                    }
                }
                
                let elapsed = start_time.elapsed();
                println!("Finished sending {}/{} catch-up chunks to client {} in {:.2}s",
                       chunks_sent, saved_chunks_count, listener_id_copy, elapsed.as_secs_f32());
            });
        }
        
        // Process broadcast messages immediately while chunks are being sent in background
        let mut chunk_count = 0;
        let mut error_count = 0;
        let mut last_activity = Instant::now();
        let mut consecutive_timeouts = 0;
        
        // Start processing new broadcast messages
        while client_connected.load(Ordering::Relaxed) {
            // Check if we've gone too long without activity
            if last_activity.elapsed() > Duration::from_secs(20) {
                println!("Client {} connection timed out due to inactivity", listener_id);
                break;
            }
            
            // Try to receive the next broadcast chunk with a SHORTER timeout (2s instead of 5s)
            // This makes the server more responsive to shutdown requests
            let receive_future = broadcast_rx.recv();
            match tokio::time::timeout(Duration::from_secs(2), receive_future).await {
                Ok(Ok(chunk)) => {
                    // Reset timeout counter on successful message
                    consecutive_timeouts = 0;
                    
                    // Update activity timestamp
                    last_activity = Instant::now();
                    
                    // Skip empty chunks (end of track marker)
                    if chunk.is_empty() {
                        println!("Client {} received end of track marker", listener_id);
                        continue;
                    }
                    
                    chunk_count += 1;
                    if chunk_count % 100 == 0 {
                        println!("Sent {} live audio chunks to client {}", chunk_count, listener_id);
                    }
                    
                    // Send binary data
                    match stream.lock().await.send(ws::Message::Binary(chunk)).await {
                        Ok(_) => {
                            // Successfully sent chunk, reset error count
                            error_count = 0;
                        },
                        Err(e) => {
                            // Error sending chunk
                            error_count += 1;
                            println!("Error sending audio chunk to client {}: {:?}", listener_id, e);
                            
                            if error_count >= 3 {
                                println!("Too many errors, closing client {} WebSocket connection", listener_id);
                                break;
                            }
                            
                            // Brief pause before trying again
                            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                        }
                    }
                },
                Ok(Err(e)) => {
                    println!("Error receiving broadcast message for client {}: {}", listener_id, e);
                    error_count += 1;
                    
                    if error_count >= 3 {
                        println!("Too many broadcast errors, closing client {} connection", listener_id);
                        break;
                    }
                    
                    // Try to resubscribe if we're lagged too far behind
                    if e.to_string().contains("lagged") {
                        println!("Client {} resubscribing to broadcast due to lag", listener_id);
                        broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                        // Brief pause to allow things to stabilize
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                Err(_) => {
                    // Timeout waiting for broadcast message
                    consecutive_timeouts += 1;
                    
                    if consecutive_timeouts >= 10 {
                        println!("Client {} had too many consecutive timeouts, closing connection", listener_id);
                        break;
                    }
                    
                    // If track ended but streaming is still active, wait for next track
                    if stream_manager_clone.track_ended() && stream_manager_clone.is_streaming() {
                        println!("Client {}: Current track ended, waiting for next track...", listener_id);
                        // Get a fresh receiver for the next track
                        broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                        // Brief pause to allow next track to start
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    
                    // If streaming stopped completely, end connection
                    if !stream_manager_clone.is_streaming() {
                        println!("Client {}: Stream has completely ended, closing connection", listener_id);
                        break;
                    }
                    
                    // Otherwise just log the timeout
                    if consecutive_timeouts % 3 == 0 {
                        println!("Client {}: Timeout waiting for broadcast message ({} consecutive)", 
                               listener_id, consecutive_timeouts);
                    }
                }
            }
        }
        
        // Cancel heartbeat task
        client_connected.store(false, Ordering::Relaxed);
        let _ = heartbeat_task.await;
        
        println!("WebSocket stream for client {} ended after sending {} broadcast chunks", 
               listener_id, chunk_count);
        
        // Decrement active listener count when done
        stream_manager_clone.decrement_listener_count();
        println!("Listener {} disconnected. Active listeners: {}", 
               listener_id, stream_manager_clone.get_active_listeners());
        
        Ok(())
    }))
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
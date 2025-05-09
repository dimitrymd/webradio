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
    Json(serde_json::json!({
        "active_listeners": stream_manager.get_active_listeners(),
        "receiver_count": stream_manager.get_receiver_count(),
        "max_concurrent_users": config::MAX_CONCURRENT_USERS
    }))
}

// WebSocket handler for streaming audio
#[get("/stream")]
pub fn stream_ws(ws: ws::WebSocket, stream_manager: &State<StreamManager>) -> ws::Channel<'static> {
    println!("WebSocket connection request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        // Return an error message and close the connection
        println!("Max concurrent users limit reached");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("Server at capacity, try again later".into())).await;
            Ok(())
        }));
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
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
        
        // Subscribe to the broadcast channel
        let mut broadcast_rx = stream_manager_clone.get_broadcast_receiver();
        
        // Send track info first
        if let Some(track_info) = stream_manager_clone.get_track_info() {
            println!("Sending track info to client: {}", track_info);
            if let Err(e) = stream.lock().await.send(ws::Message::Text(track_info)).await {
                println!("Error sending track info: {:?}", e);
                return Ok(());
            }
        }
        
        // Send ID3 header first (important for MP3 streaming)
        if let Some(id3_header) = stream_manager_clone.get_id3_header() {
            println!("Sending ID3 header to client ({} bytes)", id3_header.len());
            if let Err(e) = stream.lock().await.send(ws::Message::Binary(id3_header)).await {
                println!("Error sending ID3 header: {:?}", e);
                return Ok(());
            }
        }
        
        // Increment active listener count
        stream_manager_clone.increment_listener_count();
        println!("Listener connected. Active listeners: {}", stream_manager_clone.get_active_listeners());
        
        // Keep track of client state
        let client_connected = Arc::new(AtomicBool::new(true));
        let client_connected_clone = client_connected.clone();
        let stream_clone = stream.clone();
        
        // Set up a heartbeat task to detect disconnected clients
        let heartbeat_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            while client_connected_clone.load(Ordering::Relaxed) {
                interval.tick().await;
                if let Err(_) = stream_clone.lock().await.send(ws::Message::Ping(vec![1, 2, 3])).await {
                    // Client disconnected
                    client_connected_clone.store(false, Ordering::Relaxed);
                    break;
                }
            }
        });
        
        // Process broadcast messages
        let mut chunk_count = 0;
        let mut error_count = 0;
        let mut last_activity = Instant::now();
        
        while client_connected.load(Ordering::Relaxed) {
            // Check if we've gone too long without activity
            if last_activity.elapsed() > Duration::from_secs(10) {
                println!("Client connection timed out due to inactivity");
                break;
            }
            
            // Try to receive the next broadcast chunk with a timeout
            let receive_future = broadcast_rx.recv();
            match tokio::time::timeout(Duration::from_secs(5), receive_future).await {
                Ok(Ok(chunk)) => {
                    // Update activity timestamp
                    last_activity = Instant::now();
                    
                    // Skip empty chunks (end of track marker)
                    if chunk.is_empty() {
                        println!("Received end of track marker");
                        continue;
                    }
                    
                    chunk_count += 1;
                    if chunk_count % 100 == 0 {
                        println!("Sent {} audio chunks to client", chunk_count);
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
                            println!("Error sending audio chunk: {:?}", e);
                            
                            if error_count >= 3 {
                                println!("Too many errors, closing WebSocket connection");
                                break;
                            }
                            
                            // Brief pause before trying again
                            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                        }
                    }
                },
                Ok(Err(e)) => {
                    println!("Error receiving broadcast message: {}", e);
                    error_count += 1;
                    
                    if error_count >= 3 {
                        println!("Too many broadcast errors, closing connection");
                        break;
                    }
                    
                    // Try to resubscribe if we're lagged too far behind
                    if e.to_string().contains("lagged") {
                        println!("Resubscribing to broadcast due to lag");
                        broadcast_rx = stream_manager_clone.get_broadcast_receiver();
                    }
                    
                    // Brief pause before trying again
                    tokio::time::sleep(Duration::from_millis(100)).await;
                },
                Err(_) => {
                    // Timeout waiting for broadcast message
                    println!("Timeout waiting for broadcast message");
                    
                    // Check if streaming is still active
                    if !stream_manager_clone.is_streaming() {
                        println!("Stream has ended, closing connection");
                        break;
                    }
                    
                    // Continue waiting
                }
            }
        }
        
        // Cancel heartbeat task
        client_connected.store(false, Ordering::Relaxed);
        let _ = heartbeat_task.await;
        
        println!("WebSocket stream ended after sending {} chunks", chunk_count);
        
        // Decrement active listener count when done
        stream_manager_clone.decrement_listener_count();
        println!("Listener disconnected. Active listeners: {}", stream_manager_clone.get_active_listeners());
        
        Ok(())
    }))
}

// HTTP endpoint for direct streaming
#[get("/direct-stream")]
pub async fn direct_stream(stream_manager: &State<StreamManager>) -> Result<(ContentType, Vec<u8>), Status> {
    println!("Direct stream HTTP request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        println!("Max concurrent users limit reached");
        return Err(Status::ServiceUnavailable);
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
        println!("No tracks available for direct streaming");
        return Err(Status::NotFound);
    }
    
    // Increment active listener count
    stream_manager.increment_listener_count();
    
    // Stream the file directly if it exists
    if let Some(track) = track {
        let file_path = config::MUSIC_FOLDER.join(&track.path);
        println!("Direct stream request for track: \"{}\" by \"{}\"", track.title, track.artist);
        
        // Read the file
        match tokio::fs::read(&file_path).await {
            Ok(data) => {
                println!("Started direct streaming of file: {} ({} bytes)", file_path.display(), data.len());
                
                // Return the MP3 data with the correct content type
                Ok((ContentType::MP3, data))
            },
            Err(e) => {
                println!("Failed to read file {}: {}", file_path.display(), e);
                stream_manager.decrement_listener_count();
                Err(Status::InternalServerError)
            }
        }
    } else {
        stream_manager.decrement_listener_count();
        println!("Failed to get track for direct streaming");
        Err(Status::NotFound)
    }
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
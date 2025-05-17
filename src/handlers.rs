// Updated handlers.rs with optimized WebSocket handling

use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use rocket_ws as ws;
use rocket::futures::SinkExt; // For WebSocket streaming
use rocket::response::Stream;
use std::io::{Cursor, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use futures::stream::StreamExt;
use std::collections::VecDeque;

use log::{info, error, warn}; // Add these log macros
use tokio::sync::broadcast;   // Add broadcast import
use rocket::response::stream::Stream; // Fix Stream import
use std::sync::Arc;           // Make sure Arc is imported

use crate::models::playlist::Playlist;
use crate::services::playlist;
use crate::services::streamer::StreamManager;
use crate::services::websocket_bus::WebSocketBus;
use crate::services::transcoder::TranscoderManager;
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

// Optimized WebSocket handler that uses the shared WebSocketBus
#[get("/stream")]
pub fn stream_ws(
    ws: ws::WebSocket, 
    websocket_bus: &State<Arc<WebSocketBus>>
) -> ws::Channel<'static> {
    let websocket_bus = websocket_bus.inner().clone();
    
    ws.channel(move |stream| Box::pin(async move {
        // Add client to the bus
        let (client_id, mut msg_rx) = websocket_bus.add_client();
        
        // Send initial data to the client - fixed by making it blocking
        // Fix this line to avoid the "cannot apply unary operator" error
        if websocket_bus.send_initial_data(client_id).await == false {
            websocket_bus.remove_client(client_id);
            return Ok(());
        }
        
        // Split the stream for concurrent sending and receiving
        let (mut sink, mut stream) = stream.split();
        
        // Task that forwards messages from the bus to the client
        let forward_task = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                if let Err(e) = sink.send(msg).await {
                    log::error!("Error sending to WebSocket: {}", e);
                    break;
                }
            }
        });
        
        // Create a stream manager reference for handling client requests
        let stream_manager = websocket_bus.get_stream_manager();
        
        // Process incoming messages from client
        while let Some(result) = stream.next().await {
            match result {
                Ok(ws::Message::Close(_)) => {
                    log::debug!("Client {} sent close message", client_id);
                    break;
                },
                Ok(ws::Message::Pong(_)) => {
                    // Update last activity time
                    websocket_bus.update_client_activity(client_id);
                },
                Ok(ws::Message::Text(text)) => {
                    // Handle text commands from client
                    log::debug!("Client {} sent message: {}", client_id, text);
                    websocket_bus.update_client_activity(client_id);
                    
                    // Try to parse the message as JSON
                    if let Ok(request) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(req_type) = request.get("type").and_then(|t| t.as_str()) {
                            match req_type {
                                "now_playing_request" => {
                                    // Get current track info
                                    let track_info = get_now_playing_data(&stream_manager);
                                    
                                    // Format as a response message
                                    let response = serde_json::json!({
                                        "type": "now_playing",
                                        "track": track_info
                                    });
                                    
                                    // Send response to this client only
                                    if let Ok(response_str) = serde_json::to_string(&response) {
                                        websocket_bus.send_to_client(
                                            client_id, 
                                            ws::Message::Text(response_str)
                                        );
                                    }
                                },
                                "ping" => {
                                    // Client is checking connection - respond with pong
                                    websocket_bus.send_to_client(
                                        client_id,
                                        ws::Message::Text(r#"{"type":"pong"}"#.to_string())
                                    );
                                },
                                _ => {
                                    log::debug!("Unknown request type: {}", req_type);
                                }
                            }
                        }
                    } else {
                        log::debug!("Non-JSON message from client {}: {}", client_id, text);
                    }
                },
                Err(e) => {
                    log::error!("WebSocket error from client {}: {}", client_id, e);
                    break;
                },
                _ => {
                    // Update last activity for any message
                    websocket_bus.update_client_activity(client_id);
                }
            }
        }
        
        // Clean up
        forward_task.abort();
        websocket_bus.remove_client(client_id);
        
        Ok(())
    }))
}
#[get("/stream-opus")]
pub fn stream_opus_ws(
    ws: ws::WebSocket, 
    websocket_bus: &State<Arc<WebSocketBus>>,
    transcoder: &State<Arc<TranscoderManager>>
) -> ws::Channel<'static> {
    let websocket_bus = websocket_bus.inner().clone();
    let transcoder = transcoder.inner().clone();
    
    ws.channel(move |stream| Box::pin(async move {
        // Add client to the bus
        let (client_id, _) = websocket_bus.add_client();
        
        // Split the stream for concurrent sending and receiving
        let (mut sink, mut stream) = stream.split();
        
        // Send initial track info
        let stream_manager = websocket_bus.get_stream_manager();
        if let Some(info) = stream_manager.get_track_info() {
            if let Err(e) = sink.send(ws::Message::Text(info)).await {
                log::error!("Error sending track info: {}", e);
                websocket_bus.remove_client(client_id);
                return Ok(());
            }
        }
        
        // Get the opus broadcast receiver
        let mut opus_rx = transcoder.get_opus_broadcast_receiver();
        
        // Get initial Opus chunks and send them
        let initial_chunks = transcoder.get_opus_chunks_from_current_position();
        
        // Log the size of initial chunks for debugging
        log::info!("Sending {} initial Opus chunks to iOS client", initial_chunks.len());
        
        // Send initial chunks with enhanced error handling
        for (i, chunk) in initial_chunks.iter().enumerate() {
            if let Err(e) = sink.send(ws::Message::Binary(chunk.clone())).await {
                log::error!("Error sending initial Opus chunk {}: {}", i, e);
                websocket_bus.remove_client(client_id);
                return Ok(());
            }
            
            // Add small delays between chunks to avoid overwhelming iOS browsers
            if i % 5 == 0 && i > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }
        
        // Send a specific message to help clients verify the stream format
        let format_info = serde_json::json!({
            "type": "stream_info",
            "format": "opus",
            "container": "ogg",
            "sample_rate": 48000,
            "channels": 2
        });
        
        if let Ok(format_info_str) = serde_json::to_string(&format_info) {
            if let Err(e) = sink.send(ws::Message::Text(format_info_str)).await {
                log::error!("Error sending format info: {}", e);
            }
        }
        
        // Task that forwards messages from the bus to the client
        let forward_task = tokio::spawn(async move {
            while let Ok(chunk) = opus_rx.recv().await {
                if let Err(e) = sink.send(ws::Message::Binary(chunk)).await {
                    log::error!("Error sending Opus audio to WebSocket: {}", e);
                    break;
                }
            }
        });
        
        // Process incoming messages from client
        while let Some(result) = stream.next().await {
            match result {
                Ok(ws::Message::Close(_)) => {
                    log::debug!("Opus client {} sent close message", client_id);
                    break;
                },
                Ok(ws::Message::Pong(_)) => {
                    // Update last activity time
                    websocket_bus.update_client_activity(client_id);
                },
                Ok(ws::Message::Text(text)) => {
                    // Handle text commands from client
                    log::debug!("Opus client {} sent message: {}", client_id, text);
                    websocket_bus.update_client_activity(client_id);
                    
                    // Try to parse the message as JSON
                    if let Ok(request) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(req_type) = request.get("type").and_then(|t| t.as_str()) {
                            match req_type {
                                "now_playing_request" => {
                                    // Get current track info
                                    let track_info = get_now_playing_data(&stream_manager);
                                    
                                    // Format as a response message
                                    let response = serde_json::json!({
                                        "type": "now_playing",
                                        "track": track_info
                                    });
                                    
                                    // Send response to this client only
                                    if let Ok(response_str) = serde_json::to_string(&response) {
                                        websocket_bus.send_to_client(
                                            client_id, 
                                            ws::Message::Text(response_str)
                                        );
                                    }
                                },
                                "ping" => {
                                    // Client is checking connection - respond with pong
                                    websocket_bus.send_to_client(
                                        client_id,
                                        ws::Message::Text(r#"{"type":"pong"}"#.to_string())
                                    );
                                },
                                "format_request" => {
                                    // Client is asking about the format - send details
                                    let format_info = serde_json::json!({
                                        "type": "stream_info",
                                        "format": "opus",
                                        "container": "ogg",
                                        "sample_rate": 48000,
                                        "channels": 2
                                    });
                                    
                                    if let Ok(format_info_str) = serde_json::to_string(&format_info) {
                                        websocket_bus.send_to_client(
                                            client_id,
                                            ws::Message::Text(format_info_str)
                                        );
                                    }
                                },
                                _ => {
                                    log::debug!("Unknown request type: {}", req_type);
                                }
                            }
                        }
                    } else {
                        log::debug!("Non-JSON message from client {}: {}", client_id, text);
                    }
                },
                Err(e) => {
                    log::error!("WebSocket error from Opus client {}: {}", client_id, e);
                    break;
                },
                _ => {
                    // Update last activity for any message
                    websocket_bus.update_client_activity(client_id);
                }
            }
        }
        
        // Clean up
        forward_task.abort();
        websocket_bus.remove_client(client_id);
        
        Ok(())
    }))
}

// Direct streaming endpoint for iOS and other platforms without MSE support
#[get("/direct-stream")]
pub fn direct_stream(
    stream_manager: &State<Arc<StreamManager>>
) -> Result<Stream<impl Read>, Status> {
    // Get a reference to the StreamManager
    let sm = stream_manager.as_ref();
    
    // Check if streaming is active
    if !sm.is_streaming() {
        return Err(Status::ServiceUnavailable);
    }
    
    // Increment listener count
    sm.increment_listener_count();
    
    // Log request for debugging
    if let Some(track_info) = sm.get_track_info() {
        info!("Direct stream request, serving: {}", track_info);
    }
    
    // Get initial chunks to start with
    let (id3_header, chunks) = sm.get_chunks_from_current_position();
    
    // Create a buffer with initial data
    let mut initial_buffer = Vec::new();
    
    // Add ID3 header if available
    if let Some(header) = id3_header {
        initial_buffer.extend_from_slice(&header);
    }
    
    // Add initial chunks - use more initial chunks for better buffering
    for chunk in chunks.iter().take(config::INITIAL_CHUNKS_TO_SEND * 2) {
        if !chunk.is_empty() {
            initial_buffer.extend_from_slice(chunk);
        }
    }
    
    // Create a cursor for the initial buffer
    let initial_cursor = Cursor::new(initial_buffer);
    
    // Get a broadcast receiver for ongoing data
    let broadcast_rx = sm.get_broadcast_receiver();
    
    // Create a streaming reader that combines initial buffer with broadcast
    let stream_reader = DirectStreamReader {
        initial_data: Some(initial_cursor),
        broadcast_rx,
        stream_manager: sm.clone(), // sm.clone() already returns Arc<StreamManager>
        closed: false,
    };
    
    // Return streaming response with appropriate headers
    Ok(Stream::from(stream_reader)
       .chunked()
       .with_content_type(ContentType::MP3))
}

// Helper function to get now playing data - this would be added to handlers.rs
fn get_now_playing_data(stream_manager: &Arc<StreamManager>) -> serde_json::Value {
    // Get the actual current track from the stream manager's state
    let track_info = stream_manager.get_track_info();
    let playback_position = stream_manager.get_playback_position();
    let active_listeners = stream_manager.get_active_listeners();
    let current_bitrate = stream_manager.get_current_bitrate();
    
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
                    serde_json::Value::Number(serde_json::Number::from(current_bitrate / 1000))
                );
            }
            return track_value;
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
            
            track_json
        },
        None => serde_json::json!({
            "error": "No tracks available"
        })
    }
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

pub struct DirectStreamReader {
    initial_data: Option<Cursor<Vec<u8>>>,
    broadcast_rx: broadcast::Receiver<Vec<u8>>,
    stream_manager: Arc<StreamManager>,
    closed: bool,
}

impl Read for DirectStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // First, try to read from the initial data if available
        if let Some(ref mut cursor) = self.initial_data {
            match cursor.read(buf) {
                Ok(0) => {
                    // Initial data is exhausted, remove it
                    self.initial_data = None;
                },
                Ok(n) => {
                    // Successfully read data from initial buffer
                    return Ok(n);
                },
                Err(e) => {
                    error!("Error reading initial data: {}", e);
                    self.initial_data = None;
                    // Continue to reading from broadcast
                }
            }
        }
        
        // If closed or stream is not active, return EOF
        if self.closed || !self.stream_manager.is_streaming() {
            return Ok(0); // EOF
        }
        
        // Try to read from broadcast channel with a timeout
        // Use blocking_recv to avoid busy waiting
        match self.broadcast_rx.blocking_recv() {
            Ok(chunk) => {
                // Got a chunk, copy it to the output buffer
                let n = std::cmp::min(buf.len(), chunk.len());
                if n > 0 {
                    buf[..n].copy_from_slice(&chunk[..n]);
                }
                Ok(n)
            },
            Err(broadcast::error::RecvError::Closed) => {
                // Channel closed
                self.closed = true;
                self.stream_manager.decrement_listener_count();
                Err(std::io::Error::new(ErrorKind::BrokenPipe, "Broadcast channel closed"))
            },
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Channel lagged too much
                warn!("Broadcast channel lagged, reinitializing");
                // Get a fresh receiver and continue
                self.broadcast_rx = self.stream_manager.get_broadcast_receiver();
                // Return temporary "no data" without EOF
                Ok(0)
            }
        }
    }
}

impl Drop for DirectStreamReader {
    fn drop(&mut self) {
        if !self.closed {
            // Decrement listener count when stream is dropped
            self.stream_manager.decrement_listener_count();
            self.closed = true;
        }
    }
}
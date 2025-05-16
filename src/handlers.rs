// Updated handlers.rs with optimized WebSocket handling

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
        
        // Send initial data to the client
        if !websocket_bus.send_initial_data(client_id) {
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
        
        // Send initial chunks
        log::debug!("Sending {} initial Opus chunks", initial_chunks.len());
        for chunk in initial_chunks {
            if let Err(e) = sink.send(ws::Message::Binary(chunk)).await {
                log::error!("Error sending initial Opus chunk: {}", e);
                websocket_bus.remove_client(client_id);
                return Ok(());
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
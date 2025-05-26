// src/direct_stream.rs - True Radio implementation

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder};
use rocket::{Request, Response, State};
use std::sync::Arc;
use log::{info, error, debug};
use tokio::sync::broadcast;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::services::streamer::{StreamManager, AudioChunk};
use crate::services::playlist;
use crate::config;

// True radio streaming responder
pub struct RadioBroadcastStream {
    receiver: broadcast::Receiver<AudioChunk>,
    connection_id: String,
    platform: String,
    initial_chunks: Vec<AudioChunk>,
    current_index: usize,
}

impl RadioBroadcastStream {
    pub fn new(
        stream_manager: Arc<StreamManager>,
        platform: Option<String>,
    ) -> Result<Self, Status> {
        // Subscribe to the broadcast
        let (connection_id, receiver) = stream_manager.subscribe();
        
        let platform_str = platform.as_deref().unwrap_or("unknown");
        stream_manager.update_connection_info(&connection_id, platform_str.to_string(), String::new());
        
        info!("TRUE RADIO: New listener {} connected to broadcast on {}", 
              &connection_id[..8], platform_str);
        
        // Get recent chunks for smooth start
        let initial_chunks = stream_manager.get_recent_chunks(0);
        
        info!("TRUE RADIO: Providing {} recent chunks to new listener", initial_chunks.len());
        
        Ok(RadioBroadcastStream {
            receiver,
            connection_id,
            platform: platform_str.to_string(),
            initial_chunks,
            current_index: 0,
        })
    }
}

impl Stream for RadioBroadcastStream {
    type Item = Result<Vec<u8>, std::io::Error>;
    
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // First, send any initial chunks
        if self.current_index < self.initial_chunks.len() {
            let chunk = &self.initial_chunks[self.current_index];
            self.current_index += 1;
            return Poll::Ready(Some(Ok(chunk.data.to_vec())));
        }
        
        // Then, receive from broadcast
        match self.receiver.try_recv() {
            Ok(chunk) => {
                debug!("Sending chunk {} to listener {}", chunk.chunk_id, &self.connection_id[..8]);
                Poll::Ready(Some(Ok(chunk.data.to_vec())))
            },
            Err(broadcast::error::TryRecvError::Empty) => {
                // No data available yet, register waker
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                // We missed some chunks, but continue
                debug!("Listener {} lagged by {} chunks", &self.connection_id[..8], skipped);
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(broadcast::error::TryRecvError::Closed) => {
                info!("Broadcast closed for listener {}", &self.connection_id[..8]);
                Poll::Ready(None)
            },
        }
    }
}

impl Drop for RadioBroadcastStream {
    fn drop(&mut self) {
        info!("Listener {} disconnected from broadcast", &self.connection_id[..8]);
    }
}

// Simple streaming response
pub struct DirectStream {
    stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, std::io::Error>> + Send>>,
    headers: Vec<Header<'static>>,
    connection_id: String,
    stream_manager: Arc<StreamManager>,
}

impl DirectStream {
    pub fn new(
        stream_manager: Arc<StreamManager>,
        _requested_position: Option<u64>, // Ignored - true radio
        platform: Option<String>,
        _range_header: Option<String>, // Ignored for now
        _ios_optimized: Option<bool>,
        _chunk_size: Option<usize>,
        _initial_buffer: Option<usize>
    ) -> Result<Self, Status> {
        // Create broadcast stream
        let broadcast_stream = RadioBroadcastStream::new(stream_manager.clone(), platform.clone())?;
        let connection_id = broadcast_stream.connection_id.clone();
        let platform_str = broadcast_stream.platform.clone();
        
        // Get current track info
        let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER)
            .ok_or(Status::NotFound)?;
        
        // Build headers
        let headers = Self::build_radio_headers(&platform_str, &track, &connection_id);
        
        Ok(DirectStream {
            stream: Box::pin(broadcast_stream),
            headers,
            connection_id: connection_id.clone(),
            stream_manager,
        })
    }
    
    fn build_radio_headers(
        platform: &str,
        track: &crate::models::playlist::Track,
        connection_id: &str,
    ) -> Vec<Header<'static>> {
        let mut headers = Vec::new();
        
        // Essential headers
        headers.push(Header::new("Content-Type", "audio/mpeg"));
        headers.push(Header::new("Cache-Control", "no-cache, no-store"));
        headers.push(Header::new("Transfer-Encoding", "chunked")); // Chunked for streaming
        
        // Platform-specific
        match platform {
            "ios" => {
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("X-Accel-Buffering", "no"));
            },
            _ => {
                headers.push(Header::new("Connection", "keep-alive"));
            }
        }
        
        // CORS
        headers.push(Header::new("Access-Control-Allow-Origin", "*"));
        
        // Radio metadata
        headers.push(Header::new("X-Radio-Mode", "true-broadcast"));
        headers.push(Header::new("X-Track-Title", track.title.clone()));
        headers.push(Header::new("X-Track-Artist", track.artist.clone()));
        headers.push(Header::new("X-Connection-ID", connection_id));
        headers.push(Header::new("X-Platform", platform.to_string()));
        
        headers
    }
}

impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        // Use Rocket's streaming response
        Response::build()
            .header(ContentType::new("audio", "mpeg"))
            .raw_header("Transfer-Encoding", "chunked")
            .raw_header("Cache-Control", "no-cache")
            .streamed_body(self.stream)
            .ok()
    }
}

impl Drop for DirectStream {
    fn drop(&mut self) {
        self.stream_manager.decrement_listener_count(&self.connection_id);
    }
}

// Simplified endpoint
#[rocket::get("/direct-stream?<_position>&<platform>&<_ios_optimized>&<_chunk_size>&<_initial_buffer>&<_min_buffer_time>&<_preload>&<_buffer_recovery>")]
pub fn direct_stream(
    _position: Option<u64>,        // Ignored - true radio
    platform: Option<String>,
    _ios_optimized: Option<bool>,
    _chunk_size: Option<usize>,
    _initial_buffer: Option<usize>,
    _min_buffer_time: Option<u64>,
    _preload: Option<String>,
    _buffer_recovery: Option<u64>,
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStream, Status> {
    let platform_str = platform.as_deref().unwrap_or("unknown");
    
    info!("TRUE RADIO: Stream request from platform={}", platform_str);
    
    // Cleanup stale connections
    stream_manager.cleanup_stale_connections();
    
    // Return broadcast stream
    DirectStream::new(
        stream_manager.inner().clone(),
        None,
        platform,
        None,
        None,
        None,
        None
    )
}

// For async streaming support
pub struct RadioStreamResponse {
    stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, std::io::Error>> + Send>>,
}

impl RadioStreamResponse {
    pub fn new(stream_manager: Arc<StreamManager>) -> Self {
        let broadcast_stream = stream_manager.get_broadcast_receiver();
        
        // Convert broadcast receiver to async stream
        let stream = async_stream::stream! {
            let mut receiver = broadcast_stream;
            loop {
                match receiver.recv().await {
                    Ok(chunk) => {
                        yield Ok(chunk.data.to_vec());
                    },
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        // Continue despite lag
                        debug!("Stream lagged by {} chunks", skipped);
                        continue;
                    },
                    Err(_) => {
                        // Stream closed
                        break;
                    }
                }
            }
        };
        
        Self {
            stream: Box::pin(stream),
        }
    }
}

// Alternative async endpoint (if using Rocket with async)
#[rocket::get("/radio-stream")]
pub async fn radio_stream(
    stream_manager: &State<Arc<StreamManager>>
) -> Result<Response<'static>, Status> {
    let response = RadioStreamResponse::new(stream_manager.inner().clone());
    
    Response::build()
        .header(ContentType::new("audio", "mpeg"))
        .raw_header("Transfer-Encoding", "chunked")
        .raw_header("Cache-Control", "no-cache")
        .raw_header("X-Radio-Mode", "true-broadcast")
        .streamed_body(response.stream)
        .ok()
}

// Status endpoint
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    
    sm.cleanup_stale_connections();
    
    let active_listeners = sm.get_active_listeners();
    let (position_secs, position_ms) = sm.get_precise_position();
    
    rocket::serde::json::Json(serde_json::json!({
        "status": "streaming",
        "mode": "true_radio_broadcast",
        "active_listeners": active_listeners,
        "radio_position": position_secs,
        "radio_position_ms": position_ms,
        "single_reader": true,
        "broadcast_efficiency": "maximum",
        "memory_usage": "minimal",
        "disk_io": "single_thread"
    }))
}

#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
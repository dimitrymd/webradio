// src/direct_stream.rs - Fixed streaming implementation

use rocket::http::{ContentType, Status};
use rocket::response::{self, Responder, Response};
use rocket::{Request, State};
use std::sync::Arc;
use log::{info, debug, error};
use tokio::sync::broadcast;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::io;
use tokio::io::{AsyncRead, ReadBuf};
use bytes::Bytes;

use crate::services::streamer::{StreamManager, AudioChunk};
use crate::services::playlist;
use crate::config;

// Wrapper to convert Stream to AsyncRead
pub struct StreamToAsyncRead {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>,
    buffer: Option<Bytes>,
    buffer_pos: usize,
}

impl StreamToAsyncRead {
    pub fn new(stream: Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>) -> Self {
        Self {
            stream,
            buffer: None,
            buffer_pos: 0,
        }
    }
}

impl AsyncRead for StreamToAsyncRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            // If we have data in buffer, use it first
            if let Some(ref buffer) = self.buffer {
                if self.buffer_pos < buffer.len() {
                    let buffer_len = buffer.len();
                    let to_copy = std::cmp::min(buf.remaining(), buffer_len - self.buffer_pos);
                    buf.put_slice(&buffer[self.buffer_pos..self.buffer_pos + to_copy]);
                    self.buffer_pos += to_copy;
                    
                    if self.buffer_pos >= buffer_len {
                        self.buffer = None;
                        self.buffer_pos = 0;
                    }
                    
                    return Poll::Ready(Ok(()));
                }
            }
            
            // Get next chunk from stream
            match self.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    if bytes.is_empty() {
                        continue;
                    }
                    
                    let to_copy = std::cmp::min(buf.remaining(), bytes.len());
                    buf.put_slice(&bytes[..to_copy]);
                    
                    if to_copy < bytes.len() {
                        // Store remaining bytes for next read
                        self.buffer = Some(bytes.slice(to_copy..));
                        self.buffer_pos = 0;
                    }
                    
                    return Poll::Ready(Ok(()));
                },
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(e));
                },
                Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                },
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

// True radio streaming responder
pub struct RadioBroadcastStream {
    receiver: broadcast::Receiver<AudioChunk>,
    connection_id: String,
    platform: String,
    initial_chunks: Vec<AudioChunk>,
    current_index: usize,
    finished: bool,
    stream_manager: Arc<StreamManager>,
}

impl RadioBroadcastStream {
    pub fn new(
        stream_manager: Arc<StreamManager>,
        platform: Option<String>,
    ) -> Result<Self, Status> {
        // Subscribe to the broadcast
        let (connection_id, receiver) = stream_manager.subscribe();
        
        let platform_str = platform.as_deref().unwrap_or("unknown").to_string();
        stream_manager.update_connection_info(&connection_id, platform_str.clone(), String::new());
        
        info!("TRUE RADIO: New listener {} connected from {}", 
              &connection_id[..8], platform_str);
        
        // Get recent chunks for smooth start
        let initial_chunks = stream_manager.get_recent_chunks(0);
        
        info!("TRUE RADIO: Providing {} recent chunks to new listener", initial_chunks.len());
        
        Ok(RadioBroadcastStream {
            receiver,
            connection_id,
            platform: platform_str,
            initial_chunks,
            current_index: 0,
            finished: false,
            stream_manager,
        })
    }
}

impl Stream for RadioBroadcastStream {
    type Item = Result<Bytes, io::Error>;
    
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        
        // First, send any initial chunks
        if self.current_index < self.initial_chunks.len() {
            let chunk_data = self.initial_chunks[self.current_index].data.clone();
            self.current_index += 1;
            return Poll::Ready(Some(Ok(chunk_data)));
        }
        
        // Then, receive from broadcast
        match self.receiver.try_recv() {
            Ok(chunk) => {
                debug!("Sending chunk {} to listener {}", chunk.chunk_id, &self.connection_id[..8]);
                Poll::Ready(Some(Ok(chunk.data)))
            },
            Err(broadcast::error::TryRecvError::Empty) => {
                // No data available yet, will wake when data arrives
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                // We missed some chunks, but continue
                debug!("Listener {} lagged by {} chunks", &self.connection_id[..8], skipped);
                
                // Try to get current chunks after lag
                let recent_chunks = self.stream_manager.get_recent_chunks(0);
                if !recent_chunks.is_empty() {
                    let latest_chunk = recent_chunks.last().unwrap();
                    return Poll::Ready(Some(Ok(latest_chunk.data.clone())));
                }
                
                cx.waker().wake_by_ref();
                Poll::Pending
            },
            Err(broadcast::error::TryRecvError::Closed) => {
                info!("Broadcast closed for listener {}", &self.connection_id[..8]);
                self.finished = true;
                Poll::Ready(None)
            },
        }
    }
}

impl Drop for RadioBroadcastStream {
    fn drop(&mut self) {
        info!("Listener {} disconnected from broadcast", &self.connection_id[..8]);
        self.stream_manager.decrement_listener_count(&self.connection_id);
    }
}

// Simple streaming response
pub struct DirectStream {
    stream: StreamToAsyncRead,
    headers: Vec<(String, String)>,
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
        let platform_str = broadcast_stream.platform.clone();
        
        // Get current track info for headers
        let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER)
            .unwrap_or_else(|| crate::models::playlist::Track {
                path: "live.mp3".to_string(),
                title: "ChillOut Radio".to_string(),
                artist: "Live Stream".to_string(),
                album: "Broadcasting".to_string(),
                duration: 180,
            });
        
        // Build headers
        let headers = Self::build_radio_headers(&platform_str, &track);
        
        // Convert stream to AsyncRead
        let byte_stream: Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> = 
            Box::pin(broadcast_stream);
        let async_read = StreamToAsyncRead::new(byte_stream);
        
        Ok(DirectStream {
            stream: async_read,
            headers,
        })
    }
    
    fn build_radio_headers(
        platform: &str,
        track: &crate::models::playlist::Track,
    ) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        
        // Essential headers
        headers.push(("Content-Type".to_string(), "audio/mpeg".to_string()));
        headers.push(("Cache-Control".to_string(), "no-cache, no-store, must-revalidate".to_string()));
        headers.push(("Pragma".to_string(), "no-cache".to_string()));
        headers.push(("Expires".to_string(), "0".to_string()));
        
        // Streaming headers
        headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
        headers.push(("Connection".to_string(), "keep-alive".to_string()));
        
        // Platform-specific optimizations
        match platform {
            "ios" => {
                headers.push(("X-Accel-Buffering".to_string(), "no".to_string()));
                headers.push(("X-Content-Type-Options".to_string(), "nosniff".to_string()));
            },
            "android" => {
                headers.push(("Accept-Ranges".to_string(), "none".to_string()));
            },
            _ => {}
        }
        
        // CORS headers
        headers.push(("Access-Control-Allow-Origin".to_string(), "*".to_string()));
        headers.push(("Access-Control-Allow-Methods".to_string(), "GET, OPTIONS".to_string()));
        headers.push(("Access-Control-Expose-Headers".to_string(), "Content-Length, Content-Type".to_string()));
        
        // Radio metadata headers
        headers.push(("X-Radio-Mode".to_string(), "true-broadcast".to_string()));
        headers.push(("X-Track-Title".to_string(), track.title.clone()));
        headers.push(("X-Track-Artist".to_string(), track.artist.clone()));
        headers.push(("X-Platform".to_string(), platform.to_string()));
        headers.push(("X-Stream-Type".to_string(), "live-radio".to_string()));
        
        headers
    }
}

impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        let mut response = Response::build();
        response.header(ContentType::new("audio", "mpeg"));
        
        // Add custom headers
        for (name, value) in self.headers {
            response.raw_header(name, value);
        }
        
        // Move the stream into the response
        response.streamed_body(self.stream).ok()
    }
}

// Main streaming endpoint
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
    
    // Check if streaming is active
    if !stream_manager.is_streaming() {
        error!("Stream manager is not active");
        return Err(Status::ServiceUnavailable);
    }
    
    // Cleanup stale connections
    stream_manager.cleanup_stale_connections();
    
    // Create and return broadcast stream
    DirectStream::new(
        stream_manager.inner().clone(),
        None,
        platform,
        None,
        None,
        None,
        None
    ).map_err(|_| {
        error!("Failed to create direct stream");
        Status::InternalServerError
    })
}

// Alternative endpoint for debugging
#[rocket::get("/radio-stream")]
pub async fn radio_stream(
    stream_manager: &State<Arc<StreamManager>>
) -> rocket::serde::json::Json<serde_json::Value> {
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
        "streaming": sm.is_streaming(),
        "message": "Use /direct-stream for actual audio streaming",
        "endpoints": {
            "audio_stream": "/direct-stream",
            "now_playing": "/api/now-playing",
            "stats": "/api/stats"
        }
    }))
}

// Status endpoint
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    
    sm.cleanup_stale_connections();
    
    let active_listeners = sm.get_active_listeners();
    let (position_secs, position_ms) = sm.get_precise_position();
    
    rocket::serde::json::Json(serde_json::json!({
        "status": if sm.is_streaming() { "streaming" } else { "stopped" },
        "mode": "true_radio_broadcast",
        "active_listeners": active_listeners,
        "radio_position": position_secs,
        "radio_position_ms": position_ms,
        "streaming": sm.is_streaming(),
        "single_reader": true,
        "broadcast_efficiency": "maximum",
        "memory_usage": "minimal",
        "disk_io": "single_thread"
    }))
}

// CORS preflight for streaming
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
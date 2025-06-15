// src/direct_stream.rs - Optimized streaming implementation

use rocket::http::{ContentType, Status};
use rocket::response::{self, Responder, Response};
use rocket::{Request, State};
use std::sync::Arc;
use log::info;
use tokio::sync::broadcast;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

use crate::services::streamer::{StreamManager, AudioChunk};

// Optimized buffer for smooth streaming  
const STREAM_BUFFER_SIZE: usize = 32768; // 32KB internal buffer - balanced for low latency

pub struct AudioStream {
    receiver: broadcast::Receiver<AudioChunk>,
    connection_id: String,
    stream_manager: Arc<StreamManager>,
    current_chunk: Option<Vec<u8>>,
    chunk_position: usize,
    internal_buffer: Vec<u8>,
}

impl AudioStream {
    fn new(stream_manager: Arc<StreamManager>, platform: Option<String>) -> Result<Self, Status> {
        let (connection_id, receiver) = stream_manager.subscribe();
        let platform_str = platform.unwrap_or_else(|| "unknown".to_string());
        
        stream_manager.update_connection_info(&connection_id, platform_str, String::new());
        
        #[cfg(debug_assertions)]
        info!("New listener {} connected", &connection_id[..8]);
        
        Ok(AudioStream {
            receiver,
            connection_id,
            stream_manager,
            current_chunk: None,
            chunk_position: 0,
            internal_buffer: Vec::with_capacity(STREAM_BUFFER_SIZE),
        })
    }
}

impl AsyncRead for AudioStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // First, try to drain any buffered data
        if !self.internal_buffer.is_empty() {
            let to_copy = std::cmp::min(self.internal_buffer.len(), buf.remaining());
            buf.put_slice(&self.internal_buffer[..to_copy]);
            self.internal_buffer.drain(..to_copy);
            return Poll::Ready(Ok(()));
        }
        
        loop {
            // Handle current chunk if we have one
            if self.current_chunk.is_some() {
                let chunk_len = self.current_chunk.as_ref().unwrap().len();
                let remaining = chunk_len - self.chunk_position;
                
                if remaining > 0 {
                    let to_copy = std::cmp::min(remaining, buf.remaining());
                    let start = self.chunk_position;
                    let end = self.chunk_position + to_copy;
                    
                    buf.put_slice(&self.current_chunk.as_ref().unwrap()[start..end]);
                    self.chunk_position += to_copy;
                    
                    if self.chunk_position >= chunk_len {
                        self.current_chunk = None;
                        self.chunk_position = 0;
                    }
                    
                    return Poll::Ready(Ok(()));
                } else {
                    self.current_chunk = None;
                    self.chunk_position = 0;
                }
            }
            
            // Try to get multiple chunks at once for efficiency
            let mut chunks_received = 0;
            const MAX_CHUNKS_PER_POLL: usize = 1; // Get chunks immediately
            
            while chunks_received < MAX_CHUNKS_PER_POLL {
                match self.receiver.try_recv() {
                    Ok(audio_chunk) => {
                        // Convert Arc<[u8]> to Vec<u8> for the chunk
                        let chunk_data = audio_chunk.data.to_vec();
                        
                        if chunks_received == 0 {
                            // First chunk goes to current_chunk
                            self.current_chunk = Some(chunk_data);
                            self.chunk_position = 0;
                        } else {
                            // Additional chunks go to internal buffer
                            self.internal_buffer.extend_from_slice(&chunk_data);
                        }
                        chunks_received += 1;
                    },
                    Err(broadcast::error::TryRecvError::Empty) => {
                        if chunks_received > 0 {
                            // We got some data, process it
                            break;
                        }
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    },
                    Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                        #[cfg(debug_assertions)]
                        info!("Listener {} lagged by {} chunks", &self.connection_id[..8], skipped);
                        continue;
                    },
                    Err(broadcast::error::TryRecvError::Closed) => {
                        return Poll::Ready(Ok(()));
                    },
                }
            }
            
            // Process the chunks we received
            if chunks_received > 0 {
                continue;
            }
        }
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        info!("Listener {} disconnected", &self.connection_id[..8]);
        
        self.stream_manager.decrement_listener_count(&self.connection_id);
    }
}

// Direct streaming responder with optimized headers
pub struct DirectStreamResponse(AudioStream);

impl<'r> Responder<'r, 'static> for DirectStreamResponse {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        Response::build()
            .header(ContentType::new("audio", "mpeg"))
            .raw_header("Cache-Control", "no-cache, no-store, must-revalidate")
            .raw_header("Pragma", "no-cache")
            .raw_header("Expires", "0")
            .raw_header("Connection", "keep-alive")
            .raw_header("X-Content-Type-Options", "nosniff")
            .raw_header("Accept-Ranges", "none")
            .raw_header("Access-Control-Allow-Origin", "*")
            // TCP optimizations
            .raw_header("X-Accel-Buffering", "no")
            .raw_header("TCP-Nodelay", "1")
            .streamed_body(self.0)
            .ok()
    }
}

// Main streaming endpoint
#[rocket::get("/direct-stream?<platform>&<_t>&<_position>")]
pub async fn direct_stream(
    platform: Option<String>,
    _t: Option<u64>,
    _position: Option<u64>,
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStreamResponse, Status> {
    let sm = stream_manager.inner();
    
    // Quick check without logging
    if !sm.is_streaming() {
        return Err(Status::ServiceUnavailable);
    }
    
    // Create audio stream
    match AudioStream::new(sm.clone(), platform) {
        Ok(stream) => Ok(DirectStreamResponse(stream)),
        Err(e) => Err(e)
    }
}

// OPTIONS handler for CORS
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}

// Stream status endpoint - cached response
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    // Cache status response
    static mut LAST_STATUS: Option<(serde_json::Value, std::time::Instant)> = None;
    static mut STATUS_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());
    
    unsafe {
        let _lock = STATUS_MUTEX.lock();
        
        if let Some((ref status, ref last_time)) = LAST_STATUS {
            if last_time.elapsed().as_secs() < 1 {
                return rocket::serde::json::Json(status.clone());
            }
        }
        
        let sm = stream_manager.inner();
        let (pos_secs, pos_ms) = sm.get_precise_position();
        
        let status = serde_json::json!({
            "status": if sm.is_streaming() { "streaming" } else { "stopped" },
            "streaming": sm.is_streaming(),
            "active_listeners": sm.get_active_listeners(),
            "radio_position": pos_secs,
            "radio_position_ms": pos_ms,
            "mode": "true-radio"
        });
        
        LAST_STATUS = Some((status.clone(), std::time::Instant::now()));
        rocket::serde::json::Json(status)
    }
}

// Alternative radio stream endpoint
#[rocket::get("/radio-stream")]
pub fn radio_stream(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    rocket::serde::json::Json(serde_json::json!({
        "message": "Use /direct-stream for audio streaming",
        "streaming": stream_manager.is_streaming(),
        "active_listeners": stream_manager.get_active_listeners(),
        "endpoints": {
            "audio_stream": "/direct-stream",
            "status": "/stream-status",
            "now_playing": "/api/now-playing"
        }
    }))
}
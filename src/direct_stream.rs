// src/direct_stream.rs - Working streaming implementation

use rocket::http::{ContentType, Status};
use rocket::response::{self, Responder, Response};
use rocket::{Request, State};
use std::sync::Arc;
use log::{info, error};
use tokio::sync::broadcast;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

use crate::services::streamer::{StreamManager, AudioChunk};

// Audio stream that implements AsyncRead
pub struct AudioStream {
    receiver: broadcast::Receiver<AudioChunk>,
    connection_id: String,
    stream_manager: Arc<StreamManager>,
    current_chunk: Option<Vec<u8>>,
    chunk_position: usize,
}

impl AudioStream {
    fn new(stream_manager: Arc<StreamManager>, platform: Option<String>) -> Result<Self, Status> {
        let (connection_id, receiver) = stream_manager.subscribe();
        let platform_str = platform.unwrap_or_else(|| "unknown".to_string());
        
        stream_manager.update_connection_info(&connection_id, platform_str, String::new());
        
        info!("New listener {} connected", &connection_id[..8]);
        
        Ok(AudioStream {
            receiver,
            connection_id,
            stream_manager,
            current_chunk: None,
            chunk_position: 0,
        })
    }
}

impl AsyncRead for AudioStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            // Handle current chunk if we have one
            if self.current_chunk.is_some() {
                let chunk_len = self.current_chunk.as_ref().unwrap().len();
                let remaining = chunk_len - self.chunk_position;
                
                if remaining > 0 {
                    let to_copy = std::cmp::min(remaining, buf.remaining());
                    let start = self.chunk_position;
                    let end = self.chunk_position + to_copy;
                    
                    // Copy data to buffer
                    buf.put_slice(&self.current_chunk.as_ref().unwrap()[start..end]);
                    self.chunk_position += to_copy;
                    
                    // Check if chunk is exhausted
                    if self.chunk_position >= chunk_len {
                        self.current_chunk = None;
                        self.chunk_position = 0;
                    }
                    
                    return Poll::Ready(Ok(()));
                } else {
                    // Chunk is exhausted, clear it
                    self.current_chunk = None;
                    self.chunk_position = 0;
                }
            }
            
            // Try to get next chunk
            match self.receiver.try_recv() {
                Ok(audio_chunk) => {
                    self.current_chunk = Some(audio_chunk.data.to_vec());
                    self.chunk_position = 0;
                    // Loop back to copy data
                },
                Err(broadcast::error::TryRecvError::Empty) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                },
                Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                    info!("Listener {} lagged by {} chunks", &self.connection_id[..8], skipped);
                    // Try again
                    continue;
                },
                Err(broadcast::error::TryRecvError::Closed) => {
                    info!("Broadcast closed for listener {}", &self.connection_id[..8]);
                    return Poll::Ready(Ok(()));
                },
            }
        }
    }
}

impl Drop for AudioStream {
    fn drop(&mut self) {
        info!("Listener {} disconnected", &self.connection_id[..8]);
        self.stream_manager.decrement_listener_count(&self.connection_id);
    }
}

// Direct streaming responder
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
            .streamed_body(self.0)
            .ok()
    }
}

// Main streaming endpoint
#[rocket::get("/direct-stream?<platform>&<_t>&<_position>")]
pub async fn direct_stream(
    platform: Option<String>,
    _t: Option<u64>, // timestamp for cache busting
    _position: Option<u64>, // ignored in radio mode
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStreamResponse, Status> {
    let sm = stream_manager.inner();
    
    info!("Stream request from platform: {}", platform.as_deref().unwrap_or("unknown"));
    
    // Check if streaming is active
    if !sm.is_streaming() {
        error!("Stream manager is not active");
        return Err(Status::ServiceUnavailable);
    }
    
    // Create audio stream
    match AudioStream::new(sm.clone(), platform) {
        Ok(stream) => Ok(DirectStreamResponse(stream)),
        Err(e) => {
            error!("Failed to create audio stream");
            Err(e)
        }
    }
}

// OPTIONS handler for CORS
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}

// Stream status endpoint (JSON)
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    let (pos_secs, pos_ms) = sm.get_precise_position();
    
    rocket::serde::json::Json(serde_json::json!({
        "status": if sm.is_streaming() { "streaming" } else { "stopped" },
        "streaming": sm.is_streaming(),
        "active_listeners": sm.get_active_listeners(),
        "radio_position": pos_secs,
        "radio_position_ms": pos_ms,
        "mode": "true-radio"
    }))
}

// Alternative radio stream endpoint (for debugging)
#[rocket::get("/radio-stream")]
pub fn radio_stream(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    
    rocket::serde::json::Json(serde_json::json!({
        "message": "Use /direct-stream for audio streaming",
        "streaming": sm.is_streaming(),
        "active_listeners": sm.get_active_listeners(),
        "endpoints": {
            "audio_stream": "/direct-stream",
            "status": "/stream-status",
            "now_playing": "/api/now-playing"
        }
    }))
}
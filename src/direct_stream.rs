// direct_stream.rs - Fixed implementation for direct streaming

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::time::Instant;
use log::{info, warn, debug};

use crate::services::streamer::StreamManager;
use crate::config;

// Custom responder for direct MP3 streaming
pub struct DirectStream {
    stream_manager: Arc<StreamManager>,
}

impl DirectStream {
    pub fn new(stream_manager: Arc<StreamManager>) -> Self {
        Self { stream_manager }
    }
}

// Client info detection
struct ClientInfo {
    is_ios: bool,
    is_safari: bool,
    is_mobile: bool,
    is_slow_device: bool,
    supports_ranges: bool,
}

fn detect_client(request: &Request) -> ClientInfo {
    let user_agent = request.headers().get_one("User-Agent").unwrap_or("");
    
    let is_ios = user_agent.contains("iPhone") || user_agent.contains("iPad") || user_agent.contains("iPod");
    let is_safari = user_agent.contains("Safari") && !user_agent.contains("Chrome");
    let is_mobile = user_agent.contains("Mobile") || user_agent.contains("Android");
    let is_slow_device = is_mobile && (
        user_agent.contains("Android 5") || 
        user_agent.contains("Android 6") ||
        user_agent.contains("iPhone OS 9") ||
        user_agent.contains("iPhone OS 10")
    );
    
    // Check if client sent range header (supports ranges)
    let supports_ranges = request.headers().get_one("Range").is_some();
    
    ClientInfo {
        is_ios,
        is_safari,
        is_mobile,
        is_slow_device,
        supports_ranges,
    }
}

// Implementation of Responder for DirectStream
impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, request: &'r Request) -> response::Result<'static> {
        // Get stream manager reference
        let stream_manager = self.stream_manager;
        
        // Detect client browser and capabilities
        let client = detect_client(request);
        
        // Log connection info
        if client.is_mobile {
            info!("Direct stream request from mobile device (iOS: {}, Safari: {})", 
                  client.is_ios, client.is_safari);
        } else {
            info!("Direct stream request from desktop (Safari: {})", client.is_safari);
        }
        
        // Create response with appropriate headers
        let mut binding = Response::build();
        let mut response_builder = &mut binding;
            response_builder = response_builder
            .header(ContentType::new("audio", "mpeg"))
            .header(Header::new("Connection", "keep-alive"))
            .header(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"))
            .header(Header::new("Access-Control-Allow-Origin", "*"))
            .header(Header::new("X-Content-Type-Options", "nosniff"))
            .header(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"));
            
        // iOS and Safari specific optimizations
        if client.is_ios || client.is_safari {
            response_builder = response_builder
                .header(Header::new("Accept-Ranges", "bytes"))
                .header(Header::new("X-Accel-Buffering", "no"));
        }
            
        // Create stream body with platform-specific settings
        let initial_buffer_size = if client.is_ios {
            // iOS needs more initial buffer
            debug!("Using larger initial buffer for iOS");
            30 
        } else if client.is_mobile {
            // Mobile devices need a decent buffer
            20
        } else {
            // Desktop can start with a smaller buffer as it downloads faster
            15
        };
        
        // Finalize response with streamed body
        let response = response_builder
            .header(Header::new("Transfer-Encoding", "chunked")) // Important for streaming
            .status(Status::Ok)
            .streamed_body(DirectStreamBody { 
                stream_manager,
                start_time: Instant::now(),
                chunks_sent: 0,
                bytes_sent: 0,
                id3_header_sent: false,
                initial_chunks_sent: false,
                initial_buffer_size,
                is_ios: client.is_ios,
                is_mobile: client.is_mobile,
                last_log_time: Instant::now(),
                buffer: Vec::with_capacity(config::CHUNK_SIZE * 2),
            })
            .finalize();
        
        Ok(response)
    }
}

// Optimized stream body implementation
struct DirectStreamBody {
    stream_manager: Arc<StreamManager>,
    start_time: Instant,
    chunks_sent: usize,
    bytes_sent: usize,
    id3_header_sent: bool,
    initial_chunks_sent: bool,
    initial_buffer_size: usize,
    is_ios: bool,
    is_mobile: bool,
    last_log_time: Instant,
    buffer: Vec<u8>,
}

impl DirectStreamBody {
    // Log performance metrics
    fn log_performance_metrics(&mut self) {
        let now = Instant::now();
        
        // Only log every 30 seconds
        if now.duration_since(self.last_log_time).as_secs() >= 30 {
            let total_duration = now.duration_since(self.start_time).as_secs();
            if total_duration > 0 {
                let bytes_per_second = self.bytes_sent as f64 / total_duration as f64;
                let kbps = bytes_per_second * 8.0 / 1000.0;
                
                info!(
                    "Stream metrics: Sent {:.2} MB over {}s, {:.2} kbps, {} chunks", 
                    self.bytes_sent as f64 / (1024.0 * 1024.0),
                    total_duration,
                    kbps,
                    self.chunks_sent
                );
            }
            self.last_log_time = now;
        }
    }
}

impl rocket::tokio::io::AsyncRead for DirectStreamBody {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut rocket::tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;
        
        // Get a handle to our mutable self without fighting the borrowck
        let this = &mut *self;
        
        // If this is the first call, send ID3 header
        if !this.id3_header_sent {
            let (id3_header, _saved_chunks) = this.stream_manager.get_chunks_from_current_position();
            
            if let Some(id3) = id3_header {
                // Copy ID3 header to buffer
                this.buffer = id3.clone();
                let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
                buf.put_slice(&this.buffer[..bytes_to_copy]);
                
                // Update buffer if we didn't consume all of it
                if bytes_to_copy < this.buffer.len() {
                    this.buffer = this.buffer[bytes_to_copy..].to_vec();
                } else {
                    this.buffer.clear();
                }
                
                this.id3_header_sent = true;
                this.chunks_sent += 1;
                this.bytes_sent += bytes_to_copy;
                
                // Return if we've written anything
                if bytes_to_copy > 0 {
                    return Poll::Ready(Ok(()));
                }
            } else {
                // No ID3 header, mark as sent anyway
                this.id3_header_sent = true;
            }
        }
        
        // Send initial chunks if we haven't yet
        if !this.initial_chunks_sent {
            // Get saved chunks
            let (_, saved_chunks) = this.stream_manager.get_chunks_from_current_position();
            
            if !saved_chunks.is_empty() {
                // If we have saved chunks and the buffer is empty, fill it with the first chunk
                if this.buffer.is_empty() {
                    // Get a good amount of initial chunks
                    let start_idx = if saved_chunks.len() > this.initial_buffer_size { 
                        saved_chunks.len() - this.initial_buffer_size 
                    } else { 
                        0 
                    };
                    
                    // Add chunks to buffer
                    for chunk in saved_chunks[start_idx..].iter() {
                        if !chunk.is_empty() {
                            this.buffer.extend_from_slice(chunk);
                            this.chunks_sent += 1;
                            
                            // If buffer gets big, stop adding more chunks
                            if this.buffer.len() > config::CHUNK_SIZE * 10 {
                                break;
                            }
                        }
                    }
                    
                    debug!("Initial buffer filled with {} bytes from {} chunks", 
                          this.buffer.len(), this.chunks_sent - 1); // -1 because ID3 was counted
                }
                
                // If we have data in the buffer, send it
                if !this.buffer.is_empty() {
                    let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
                    buf.put_slice(&this.buffer[..bytes_to_copy]);
                    this.bytes_sent += bytes_to_copy;
                    
                    // Update buffer if we didn't consume all of it
                    if bytes_to_copy < this.buffer.len() {
                        this.buffer = this.buffer[bytes_to_copy..].to_vec();
                    } else {
                        this.buffer.clear();
                        this.initial_chunks_sent = true; // Mark initial chunks as sent if buffer is empty
                    }
                    
                    return Poll::Ready(Ok(()));
                } else {
                    // No data in buffer, mark initial chunks as sent
                    this.initial_chunks_sent = true;
                }
            } else {
                // No saved chunks, mark as sent anyway
                this.initial_chunks_sent = true;
            }
        }
        
        // After initial data, if buffer still has data, send it
        if !this.buffer.is_empty() {
            let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
            buf.put_slice(&this.buffer[..bytes_to_copy]);
            this.bytes_sent += bytes_to_copy;
            
            // Update buffer if we didn't consume all of it
            if bytes_to_copy < this.buffer.len() {
                this.buffer = this.buffer[bytes_to_copy..].to_vec();
            } else {
                this.buffer.clear();
            }
            
            return Poll::Ready(Ok(()));
        }
        
        // Get a broadcast receiver to receive new chunks
        let mut broadcast_rx = this.stream_manager.get_broadcast_receiver();
        
        // Poll for new audio chunks
        match broadcast_rx.try_recv() {
            Ok(chunk) => {
                // We got a chunk, send it
                if !chunk.is_empty() {
                    let bytes_to_copy = std::cmp::min(chunk.len(), buf.remaining());
                    buf.put_slice(&chunk[..bytes_to_copy]);
                    this.chunks_sent += 1;
                    this.bytes_sent += bytes_to_copy;
                    
                    // If we didn't copy the whole chunk, save the rest
                    if bytes_to_copy < chunk.len() {
                        this.buffer = chunk[bytes_to_copy..].to_vec();
                    }
                    
                    // Log performance occasionally
                    this.log_performance_metrics();
                    
                    return Poll::Ready(Ok(()));
                }
                
                // If chunk was empty but it's JSON (track info), just continue
                if let Ok(_) = serde_json::from_slice::<serde_json::Value>(&chunk) {
                    // This was JSON metadata, keep waiting for audio
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                
                // If chunk was empty for other reasons, try again immediately
                cx.waker().wake_by_ref();
                return Poll::Pending;
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // No data available right now, wait for more
                cx.waker().wake_by_ref();
                return Poll::Pending;
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                // Channel closed, end stream
                warn!("Broadcast channel closed, ending direct stream");
                return Poll::Ready(Ok(()));
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                // We lagged behind, log and get a new receiver
                warn!("Direct stream lagged behind by {} messages", skipped);
                
                // Reset receiver
                let _ = this.stream_manager.get_broadcast_receiver();
                
                // Keep polling
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        }
    }
}

// Handler for the direct stream endpoint
#[rocket::get("/direct-stream")]
pub fn direct_stream(stream_manager: &State<Arc<StreamManager>>) -> DirectStream {
    // Update listener count
    stream_manager.increment_listener_count();
    
    // Return the direct stream handler
    DirectStream::new(stream_manager.inner().clone())
}

// Add additional route to help with browser quirks
#[rocket::head("/direct-stream")]
pub fn direct_stream_head(stream_manager: &State<Arc<StreamManager>>) -> response::status::Accepted<&'static str> {
    // Some browsers send HEAD requests before streaming
    response::status::Accepted("Audio stream available")
}

// Add route for checking stream status (for player healthchecks)
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    let is_streaming = sm.is_streaming();
    let active_listeners = sm.get_active_listeners();
    let _current_track = sm.get_track_info();
    
    let status = serde_json::json!({
        "status": if is_streaming { "streaming" } else { "stopped" },
        "active_listeners": active_listeners,
        "stream_available": true,
        "server_time": chrono::Local::now().to_rfc3339()
    });
    
    rocket::serde::json::Json(status)
}
// direct_stream.rs - Improved implementation for direct streaming with position synchronization

use rocket::http::{ContentType, Header, Status, uri::Query};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::time::{Duration, Instant};
use log::{info, warn, debug, error};

use crate::services::streamer::StreamManager;
use crate::config;

// Custom responder for direct MP3 streaming
pub struct DirectStream {
    stream_manager: Arc<StreamManager>,
    requested_position: Option<u64>,
    is_ios: bool,
    is_safari: bool,
    is_mobile: bool,
    requested_large_buffer: bool,
}

impl DirectStream {
    pub fn new(stream_manager: Arc<StreamManager>, requested_position: Option<u64>, is_ios: bool, is_safari: bool, is_mobile: bool, requested_large_buffer: bool) -> Self {
        Self { 
            stream_manager,
            requested_position,
            is_ios,
            is_safari,
            is_mobile,
            requested_large_buffer
        }
    }
}

// Implementation of Responder for DirectStream
impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, request: &'r Request) -> response::Result<'static> {
        // Get stream manager reference
        let stream_manager = self.stream_manager;
        
        // Log connection info with more details
        if self.is_mobile {
            info!("Direct stream request from mobile device (iOS: {}, Safari: {}, Position: {:?})", 
                  self.is_ios, self.is_safari, self.requested_position);
        } else {
            info!("Direct stream request from desktop (Safari: {}, Position: {:?})", 
                  self.is_safari, self.requested_position);
        }
        
        // Get current server position for synchronization
        let server_position = self.requested_position.unwrap_or_else(|| stream_manager.get_playback_position());
        info!("Starting stream at position {}s", server_position);
        
        // Create response with appropriate headers
        let mut binding = Response::build();
        let response_builder = &mut binding;
        
        // Add required headers
        response_builder.header(ContentType::new("audio", "mpeg"))
            .header(Header::new("Connection", "keep-alive"))
            .header(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"))
            .header(Header::new("Access-Control-Allow-Origin", "*"))
            .header(Header::new("X-Content-Type-Options", "nosniff"))
            .header(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"))
            .header(Header::new("X-Server-Position", server_position.to_string()));
            
        // iOS and Safari specific optimizations
        if self.is_ios || self.is_safari {
            response_builder
                .header(Header::new("Accept-Ranges", "bytes"))
                .header(Header::new("X-Accel-Buffering", "no"));
        }
        
        // Add explicit content length for some browsers if we know the playlist duration
        if let Some(track) = crate::services::playlist::get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
            if track.duration > 0 {
                response_builder.header(Header::new("X-Content-Duration", track.duration.to_string()));
                    
                // For iOS devices, setting a large content length can help with buffering
                if self.is_ios {
                    // Estimate byte size based on bitrate (assuming 128kbps average)
                    let estimated_bytes = track.duration * 16000; // 16KB per second
                    response_builder.header(Header::new("X-Content-Length-Hint", estimated_bytes.to_string()));
                }
            }
        }
            
        // Create stream body with platform-specific settings
        let initial_buffer_size = if self.is_ios || self.requested_large_buffer {
            // iOS needs much more initial buffer
            debug!("Using extra large initial buffer for iOS");
            60 // Significantly increased from 30
        } else if self.is_safari {
            // Safari needs more buffer than Chrome
            45
        } else if self.is_mobile {
            // Mobile devices need a decent buffer
            30 // Increased from 20
        } else {
            // Desktop can start with a smaller buffer as it downloads faster
            20 // Increased from 15
        };
        
        // Finalize response with streamed body
        let response = response_builder
            .header(Header::new("Transfer-Encoding", "chunked")) // Important for streaming
            .status(Status::Ok)
            .streamed_body(DirectStreamBody { 
                stream_manager,
                server_position,
                start_time: Instant::now(),
                chunks_sent: 0,
                bytes_sent: 0,
                id3_header_sent: false,
                initial_chunks_sent: false,
                initial_buffer_size,
                is_ios: self.is_ios,
                is_safari: self.is_safari,
                is_mobile: self.is_mobile,
                requested_large_buffer: self.requested_large_buffer,
                last_log_time: Instant::now(),
                buffer: Vec::with_capacity(config::CHUNK_SIZE * 4), // Increased buffer capacity
                send_delay: if self.is_ios { Duration::from_millis(40) } else { Duration::from_millis(20) },
                last_send_time: Instant::now(),
                receiver: None, // Will be initialized in poll_read
                skipped_initial_chunks: false,
                has_broadcast_receiver: false,
                lagged_count: 0,
            })
            .finalize();
        
        Ok(response)
    }
}

// Optimized stream body implementation
struct DirectStreamBody {
    stream_manager: Arc<StreamManager>,
    server_position: u64,
    start_time: Instant,
    chunks_sent: usize,
    bytes_sent: usize,
    id3_header_sent: bool,
    initial_chunks_sent: bool,
    initial_buffer_size: usize,
    is_ios: bool,
    is_safari: bool,
    is_mobile: bool,
    requested_large_buffer: bool,
    last_log_time: Instant,
    buffer: Vec<u8>,
    send_delay: Duration,
    last_send_time: Instant,
    receiver: Option<tokio::sync::broadcast::Receiver<Vec<u8>>>,
    skipped_initial_chunks: bool,
    has_broadcast_receiver: bool,
    lagged_count: usize,
}

impl DirectStreamBody {
    // Initialize the broadcast receiver
    fn ensure_broadcast_receiver(&mut self) -> bool {
        if self.has_broadcast_receiver {
            return true;
        }
        
        self.receiver = Some(self.stream_manager.get_broadcast_receiver());
        self.has_broadcast_receiver = true;
        true
    }
    
    // Get data from broadcast channel with better error handling
    fn get_next_chunk(&mut self) -> Option<Vec<u8>> {
        if !self.has_broadcast_receiver {
            if !self.ensure_broadcast_receiver() {
                return None;
            }
        }
        
        let receiver = self.receiver.as_mut()?;
        
        match receiver.try_recv() {
            Ok(chunk) => {
                // Successfully received a chunk
                Some(chunk)
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // No data available yet, that's normal
                None
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                // Channel closed, this is a fatal error
                warn!("Broadcast channel closed, ending stream");
                self.has_broadcast_receiver = false;
                None
            },
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                // We're lagging behind, get a new receiver
                warn!("Stream lagged behind by {} messages (total lags: {})", 
                      skipped, self.lagged_count + 1);
                
                self.lagged_count += 1;
                
                // Get a fresh receiver
                self.receiver = Some(self.stream_manager.get_broadcast_receiver());
                
                // If we've lagged too many times, this is a sign of a serious problem
                if self.lagged_count > 5 {
                    error!("Too many lags ({}) - stream may be corrupted", self.lagged_count);
                    // Consider adding a special error marker to the stream here
                }
                
                // Try again immediately with the new receiver
                match self.receiver.as_mut().unwrap().try_recv() {
                    Ok(chunk) => Some(chunk),
                    _ => None
                }
            }
        }
    }
    
    // Load saved chunks, skip to approximate position
    fn load_saved_chunks(&mut self) -> Vec<Vec<u8>> {
        let (id3_header, saved_chunks) = self.stream_manager.get_chunks_from_current_position();
        
        // Make sure we've sent the ID3 header first
        if !self.id3_header_sent && id3_header.is_some() {
            self.buffer.extend_from_slice(&id3_header.unwrap());
            self.id3_header_sent = true;
        }
        
        // Return the saved chunks for further processing
        saved_chunks
    }
    
    // Skip chunks to reach the requested position
    fn skip_to_position(&mut self, saved_chunks: &[Vec<u8>]) -> usize {
        // If no position specified or at start, don't skip anything
        if self.server_position == 0 || saved_chunks.is_empty() {
            return 0;
        }
        
        // Estimate chunks to skip based on position
        // This is a rough estimate - each chunk is ~0.5-1 second of audio depending on bitrate
        let target_chunk = std::cmp::min(
            self.server_position as usize * 2, // Use a conservative estimate of 2 chunks per second
            saved_chunks.len().saturating_sub(self.initial_buffer_size)
        );
        
        if target_chunk > 0 {
            info!("Skipping approximately {} chunks to reach position {}s", 
                 target_chunk, self.server_position);
            return target_chunk;
        }
        
        0
    }
    
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
                    "Stream metrics: Sent {:.2} MB over {}s, {:.2} kbps, {} chunks, lags: {}", 
                    self.bytes_sent as f64 / (1024.0 * 1024.0),
                    total_duration,
                    kbps,
                    self.chunks_sent,
                    self.lagged_count
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
        
        // Implement rate limiting for smoother streaming
        let now = Instant::now();
        if now.duration_since(this.last_send_time) < this.send_delay && !this.buffer.is_empty() {
            // We're sending too fast, wait a bit
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        
        // If we have data in buffer, send it without fetching more
        if !this.buffer.is_empty() {
            let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
            buf.put_slice(&this.buffer[..bytes_to_copy]);
            this.bytes_sent += bytes_to_copy;
            
            // Update buffer if we didn't consume all of it - more efficient
            if bytes_to_copy < this.buffer.len() {
                // Create a more efficient buffer update that doesn't reallocate
                let remaining = this.buffer.split_off(bytes_to_copy);
                this.buffer = remaining;
            } else {
                this.buffer.clear();
            }
            
            this.last_send_time = now;
            return Poll::Ready(Ok(()));
        }
        
        // If this is the first call, set up initial state
        if !this.id3_header_sent || !this.initial_chunks_sent {
            // Get saved chunks
            let saved_chunks = this.load_saved_chunks();
            
            if !this.skipped_initial_chunks && this.server_position > 0 {
                // Try to skip to the requested position if needed
                let skip_chunks = this.skip_to_position(&saved_chunks);
                
                // Add appropriate chunks to buffer
                let start_idx = skip_chunks;
                let end_idx = saved_chunks.len();
                
                // Fill buffer with appropriate chunks
                for chunk in saved_chunks[start_idx..end_idx].iter() {
                    if !chunk.is_empty() {
                        this.buffer.extend_from_slice(chunk);
                        this.chunks_sent += 1;
                        
                        // If buffer gets very big, stop adding more chunks
                        let max_buffer = if this.is_ios || this.requested_large_buffer {
                            config::CHUNK_SIZE * 15
                        } else if this.is_safari {
                            config::CHUNK_SIZE * 12
                        } else {
                            config::CHUNK_SIZE * 10
                        };
                        
                        if this.buffer.len() > max_buffer {
                            break;
                        }
                    }
                }
                
                this.skipped_initial_chunks = true;
                
                // If we have data, process it
                if !this.buffer.is_empty() {
                    debug!("Initial buffer filled with {} bytes after skipping {} chunks", 
                           this.buffer.len(), skip_chunks);
                           
                    let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
                    buf.put_slice(&this.buffer[..bytes_to_copy]);
                    this.bytes_sent += bytes_to_copy;
                    this.last_send_time = now;
                    
                    // Update buffer efficiently
                    if bytes_to_copy < this.buffer.len() {
                        let remaining = this.buffer.split_off(bytes_to_copy);
                        this.buffer = remaining;
                    } else {
                        this.buffer.clear();
                    }
                    
                    // If we've sent enough data, mark initial chunks as sent
                    if this.chunks_sent >= this.initial_buffer_size {
                        this.initial_chunks_sent = true;
                        debug!("Initial buffer sent after position skipping, switching to streaming mode");
                    }
                    
                    return Poll::Ready(Ok(()));
                }
            } else if !this.initial_chunks_sent && !saved_chunks.is_empty() {
                // Fill buffer with initial chunks
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
                        
                        // If buffer gets very big, stop adding more chunks
                        let max_buffer = if this.is_ios || this.requested_large_buffer {
                            config::CHUNK_SIZE * 15
                        } else if this.is_safari {
                            config::CHUNK_SIZE * 12
                        } else {
                            config::CHUNK_SIZE * 10
                        };
                        
                        if this.buffer.len() > max_buffer {
                            break;
                        }
                    }
                }
                
                debug!("Standard initial buffer filled with {} bytes from {} chunks", 
                      this.buffer.len(), this.chunks_sent);
                
                // If we're on iOS or requested large buffer, log more detailed info
                if this.is_ios || this.requested_large_buffer {
                    info!("iOS device: Prepared large initial buffer of {} bytes from {} chunks", 
                          this.buffer.len(), this.chunks_sent);
                }
                
                // If we have data in buffer, send it
                if !this.buffer.is_empty() {
                    let bytes_to_copy = std::cmp::min(this.buffer.len(), buf.remaining());
                    buf.put_slice(&this.buffer[..bytes_to_copy]);
                    this.bytes_sent += bytes_to_copy;
                    this.last_send_time = now;
                    
                    // Update buffer efficiently
                    if bytes_to_copy < this.buffer.len() {
                        let remaining = this.buffer.split_off(bytes_to_copy);
                        this.buffer = remaining;
                    } else {
                        this.buffer.clear();
                        
                        // Mark initial chunks as sent if we've sent enough
                        if this.chunks_sent >= this.initial_buffer_size / 2 {
                            this.initial_chunks_sent = true;
                            debug!("Standard initial buffer sent, switching to streaming mode");
                        }
                    }
                    
                    return Poll::Ready(Ok(()));
                } else {
                    // No data in buffer, mark initial chunks as sent
                    this.initial_chunks_sent = true;
                }
            } else {
                // No saved chunks or already handled, mark as complete
                this.initial_chunks_sent = true;
            }
        }
        
        // Ensure we have a broadcast receiver
        if !this.has_broadcast_receiver {
            this.ensure_broadcast_receiver();
        }
        
        // Try to get the next audio chunk
        if let Some(chunk) = this.get_next_chunk() {
            // We got a chunk, check if it's JSON metadata or actual audio
            if !chunk.is_empty() {
                // Try to parse as JSON first
                if let Ok(_) = serde_json::from_slice::<serde_json::Value>(&chunk) {
                    // This is JSON metadata, not audio data
                    // Just continue and get the next chunk
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                
                // This is audio data, send it
                let bytes_to_copy = std::cmp::min(chunk.len(), buf.remaining());
                buf.put_slice(&chunk[..bytes_to_copy]);
                this.chunks_sent += 1;
                this.bytes_sent += bytes_to_copy;
                this.last_send_time = now;
                
                // If we didn't copy the whole chunk, save the rest
                if bytes_to_copy < chunk.len() {
                    this.buffer = chunk[bytes_to_copy..].to_vec();
                }
                
                // Log performance occasionally
                this.log_performance_metrics();
                
                return Poll::Ready(Ok(()));
            } else {
                // Empty chunk, try again
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        } else {
            // No data available, wait for more
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
    }
}

// Helper function to detect device info from URL query parameters
fn detect_platform_from_query(query: Option<&str>) -> (bool, bool, bool, bool) {
    let mut is_ios = false;
    let mut is_safari = false;
    let mut is_mobile = false;
    let mut large_buffer = false;
    
    if let Some(query_str) = query {
        // Parse the query string
        if query_str.contains("platform=ios") {
            is_ios = true;
            is_mobile = true;
        }
        
        if query_str.contains("platform=safari") {
            is_safari = true;
        }
        
        if query_str.contains("platform=mobile") {
            is_mobile = true;
        }
        
        if query_str.contains("buffer=large") {
            large_buffer = true;
        }
    }
    
    (is_ios, is_safari, is_mobile, large_buffer)
}

// Handler for the direct stream endpoint
#[rocket::get("/direct-stream?<position>&<platform>&<buffer>")]
pub fn direct_stream(
    position: Option<u64>,
    platform: Option<&str>,
    buffer: Option<&str>,
    stream_manager: &State<Arc<StreamManager>>
) -> DirectStream {
    // Update listener count
    stream_manager.increment_listener_count();
    
    // Detect platform
    let is_ios = platform.map_or(false, |p| p == "ios");
    let is_safari = platform.map_or(false, |p| p == "safari");
    let is_mobile = platform.map_or(false, |p| p == "mobile") || is_ios;
    let large_buffer = buffer.map_or(false, |b| b == "large");
    
    // Return the direct stream handler with position
    DirectStream::new(
        stream_manager.inner().clone(), 
        position,
        is_ios,
        is_safari,
        is_mobile,
        large_buffer
    )
}

// Range request support for more precise seeking
#[rocket::get("/direct-stream/range?<bytes>&<platform>&<buffer>")]
pub fn direct_stream_range(
    bytes: Option<u64>,
    platform: Option<&str>,
    buffer: Option<&str>,
    stream_manager: &State<Arc<StreamManager>>
) -> DirectStream {
    // Convert byte position to stream position (very rough estimate)
    // This depends on the bitrate of your audio
    let position = bytes.map(|b| b / 16000); // Assuming 128kbps = 16KB/sec
    
    if let Some(pos) = position {
        info!("Range request: bytes={}, pos={}s", bytes.unwrap_or(0), pos);
    }
    
    // Detect platform
    let is_ios = platform.map_or(false, |p| p == "ios");
    let is_safari = platform.map_or(false, |p| p == "safari");
    let is_mobile = platform.map_or(false, |p| p == "mobile") || is_ios;
    let large_buffer = buffer.map_or(false, |b| b == "large");
    
    // Update listener count
    stream_manager.increment_listener_count();
    
    // Return the direct stream handler with position
    DirectStream::new(
        stream_manager.inner().clone(), 
        position,
        is_ios,
        is_safari,
        is_mobile,
        large_buffer
    )
}

// Add additional route to help with browser quirks
#[rocket::head("/direct-stream")]
pub fn direct_stream_head(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    // Some browsers send HEAD requests before streaming
    // Return metadata about the current track
    let active_listeners = stream_manager.get_active_listeners();
    let current_bitrate = stream_manager.get_current_bitrate() / 1000; // Convert to kbps
    let current_position = stream_manager.get_playback_position();
    
    // Get current track info
    let track_info = if let Some(track) = crate::services::playlist::get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration
        })
    } else {
        serde_json::json!({ "error": "No track available" })
    };
    
    rocket::serde::json::Json(serde_json::json!({
        "status": "available",
        "listeners": active_listeners,
        "bitrate": current_bitrate,
        "current_position": current_position,
        "current_track": track_info,
        "stream_url": "/direct-stream",
        "server_time": chrono::Local::now().to_rfc3339()
    }))
}

// Add route for checking stream status (for player healthchecks)
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    let is_streaming = sm.is_streaming();
    let active_listeners = sm.get_active_listeners();
    let current_bitrate = sm.get_current_bitrate() / 1000; // Convert to kbps
    let playback_position = sm.get_playback_position();
    
    // Get detailed track info
    let track_info = if let Some(track) = crate::services::playlist::get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "position": playback_position
        })
    } else {
        serde_json::json!(null)
    };
    
    let status = serde_json::json!({
        "status": if is_streaming { "streaming" } else { "stopped" },
        "active_listeners": active_listeners,
        "stream_available": true,
        "playback_position": playback_position,
        "bitrate_kbps": current_bitrate,
        "current_track": track_info,
        "server_time": chrono::Local::now().to_rfc3339()
    });
    
    rocket::serde::json::Json(status)
}
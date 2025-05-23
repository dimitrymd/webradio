// src/direct_stream.rs - Fixed version with Android position synchronization

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use log::{info, error, warn};

use crate::services::streamer::StreamManager;
use crate::services::playlist;
use crate::config;

// Simple streaming responder that works with synchronous I/O
pub struct DirectStream {
    data: Vec<u8>,
    content_type: ContentType,
    headers: Vec<Header<'static>>,
    status: Status,
}

impl DirectStream {
    pub fn new(
        stream_manager: Arc<StreamManager>, 
        requested_position: Option<u64>,
        platform: Option<String>,
        range_header: Option<String>
    ) -> Result<Self, Status> {
        // Get the current track from the playlist
        let track = match playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            Some(track) => track,
            None => {
                error!("No track available to stream");
                return Err(Status::NotFound);
            }
        };
        
        info!("Position-synchronized streaming: {} by {} (platform: {:?})", 
              track.title, track.artist, platform);
        
        // FIXED: Always use server position as authoritative source
        let (server_position_secs, server_position_ms) = stream_manager.get_precise_position();
        
        // Enhanced platform detection
        let is_android = platform.as_deref().map(|p| 
            p.contains("android") || p.contains("Android")
        ).unwrap_or(false);
        
        let is_ios = platform.as_deref() == Some("ios") || platform.as_deref() == Some("safari");
        
        // ANDROID FIX: Only use requested position if it's very close to server position
        // This prevents Android from using stale cached positions
        let server_position = if let Some(req_pos) = requested_position {
            let diff = (req_pos as i64 - server_position_secs as i64).abs();
            
            if is_android {
                // Android: Be more strict about position validation
                if diff <= 3 {
                    info!("Android: Using requested position {}s (close to server {}s)", req_pos, server_position_secs);
                    req_pos
                } else {
                    warn!("Android: Requested position {}s differs too much from server {}s, using server position", 
                          req_pos, server_position_secs);
                    server_position_secs
                }
            } else {
                // Other platforms: More lenient
                if diff <= 5 {
                    info!("Using requested position {}s (close to server {}s)", req_pos, server_position_secs);
                    req_pos
                } else {
                    warn!("Requested position {}s differs too much from server {}s, using server position", 
                          req_pos, server_position_secs);
                    server_position_secs
                }
            }
        } else {
            info!("No requested position, using server position: {}s", server_position_secs);
            server_position_secs
        };
        
        if is_android {
            info!("Android client - Server pos: {}s+{}ms, Requested: {:?}, Final: {}s", 
                  server_position_secs, server_position_ms, requested_position, server_position);
        }
        
        // Create the full path to the MP3 file
        let file_path = config::MUSIC_FOLDER.join(&track.path);
        
        if !file_path.exists() {
            error!("Track file not found: {}", file_path.display());
            return Err(Status::NotFound);
        }
        
        // Get file metadata
        let file_metadata = match std::fs::metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(e) => {
                error!("Error getting file metadata: {}", e);
                return Err(Status::InternalServerError);
            }
        };
        
        let file_size = file_metadata.len();
        
        // Calculate more accurate bitrate
        let bitrate_kbps = Self::calculate_accurate_bitrate(&file_path, &track, file_size);
        
        info!("Streaming with position: {}s, bitrate: {}kbps, file size: {} bytes", 
              server_position, bitrate_kbps, file_size);
        
        // Determine start and end bytes based on range header or position
        let (start_byte, end_byte, is_partial) = if let Some(range) = &range_header {
            Self::parse_range_header(range, file_size)
        } else {
            // Use improved position-to-byte conversion
            let byte_position = Self::time_to_bytes_improved(
                server_position, 
                bitrate_kbps, 
                file_size,
                &file_path
            );
            
            let start = if byte_position >= file_size { 
                warn!("Calculated position {}s (byte {}) exceeds file size {}, starting from beginning", 
                      server_position, byte_position, file_size);
                0 
            } else { 
                byte_position 
            };
            
            info!("Position sync: {}s -> byte {} of {} (bitrate: {}kbps)", 
                  server_position, start, file_size, bitrate_kbps);
            (start, file_size - 1, false)
        };
        
        // Open the file
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Error opening file: {}", e);
                return Err(Status::InternalServerError);
            }
        };
        
        // Seek to start position if needed
        if start_byte > 0 {
            if let Err(e) = file.seek(SeekFrom::Start(start_byte)) {
                error!("Error seeking to position in file: {}", e);
                return Err(Status::InternalServerError);
            }
        }
        
        // Calculate content length for this response
        let content_length = end_byte - start_byte + 1;
        
        // Determine chunk size based on platform and bitrate
        let chunk_size = Self::calculate_optimal_chunk_size(&platform, bitrate_kbps, content_length);
        let mut buffer = vec![0u8; chunk_size];
        
        let bytes_read = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(e) => {
                error!("Error reading file: {}", e);
                return Err(Status::InternalServerError);
            }
        };
        
        // Truncate buffer to actual bytes read
        buffer.truncate(bytes_read);
        
        // Build headers
        let mut headers = Vec::new();
        
        // Essential headers for audio streaming
        headers.push(Header::new("Accept-Ranges", "bytes"));
        headers.push(Header::new("Content-Length", bytes_read.to_string()));
        
        // Platform-specific headers
        if is_android {
            // Android-specific optimizations
            headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
            headers.push(Header::new("Pragma", "no-cache"));
            headers.push(Header::new("Connection", "keep-alive"));
            headers.push(Header::new("X-Android-Optimized", "true"));
        } else if is_ios {
            // iOS prefers some caching for smoother playback
            headers.push(Header::new("Cache-Control", "public, max-age=3600"));
            headers.push(Header::new("Connection", "keep-alive"));
        } else {
            // Other browsers - minimal caching for more responsive position sync
            headers.push(Header::new("Cache-Control", "public, max-age=30"));
            headers.push(Header::new("Pragma", "no-cache"));
        }
        
        // CORS headers for web player compatibility
        headers.push(Header::new("Access-Control-Allow-Origin", "*"));
        headers.push(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"));
        headers.push(Header::new("Access-Control-Allow-Headers", "Range"));
        headers.push(Header::new("X-Content-Type-Options", "nosniff"));
        
        // Enhanced debug headers for troubleshooting
        headers.push(Header::new("X-Track-Title", track.title.clone()));
        headers.push(Header::new("X-Track-Duration", track.duration.to_string()));
        headers.push(Header::new("X-Bitrate-Kbps", bitrate_kbps.to_string()));
        headers.push(Header::new("X-Start-Byte", start_byte.to_string()));
        headers.push(Header::new("X-Content-Length", bytes_read.to_string()));
        headers.push(Header::new("X-Streaming-Method", "position_sync_chunked"));
        headers.push(Header::new("X-Server-Position", server_position_secs.to_string()));
        headers.push(Header::new("X-Server-Position-Ms", server_position_ms.to_string()));
        headers.push(Header::new("X-Position-Used", server_position.to_string()));
        headers.push(Header::new("X-Position-Source", 
            if requested_position.is_some() { "validated_request" } else { "server_authoritative" }));
        
        if is_android {
            headers.push(Header::new("X-Android-Debug", "position_sync_fixed"));
            headers.push(Header::new("X-Android-Position-Validation", 
                if requested_position.is_some() { "strict" } else { "server_only" }));
        }
        
        // Set appropriate status code
        let status = if is_partial {
            headers.push(Header::new("Content-Range", 
                format!("bytes {}-{}/{}", start_byte, start_byte + bytes_read as u64 - 1, file_size)));
            Status::PartialContent
        } else {
            Status::Ok
        };
        
        Ok(DirectStream {
            data: buffer,
            content_type: ContentType::new("audio", "mpeg"),
            headers,
            status,
        })
    }
    
    // Improved time-to-bytes conversion with better accuracy
    fn time_to_bytes_improved(
        position_seconds: u64, 
        bitrate_kbps: u64, 
        file_size: u64,
        file_path: &std::path::Path
    ) -> u64 {
        if bitrate_kbps == 0 || position_seconds == 0 {
            return 0;
        }
        
        // Detect actual ID3 tag size from file
        let id3_offset = Self::detect_id3_size(file_path).unwrap_or(1024);
        
        // Convert time to bytes with better precision
        let bytes_per_second = (bitrate_kbps * 1000) / 8;
        let byte_position = id3_offset + (position_seconds * bytes_per_second);
        
        // Ensure we don't exceed file boundaries
        let max_position = file_size.saturating_sub(config::CHUNK_SIZE as u64);
        
        // Align to MP3 frame boundaries (144 bytes for most MP3s at 44.1kHz)
        // This is much more accurate than 1KB alignment
        let frame_size = Self::calculate_mp3_frame_size(bitrate_kbps);
        let aligned_position = (byte_position / frame_size) * frame_size;
        
        let final_position = aligned_position.min(max_position);
        
        info!("Position calculation: {}s -> {}B (ID3: {}B, align: {}B, max: {}B)", 
              position_seconds, final_position, id3_offset, frame_size, max_position);
        
        final_position
    }
    
    // Calculate MP3 frame size for better alignment
    fn calculate_mp3_frame_size(bitrate_kbps: u64) -> u64 {
        // Standard MP3 frame calculation for 44.1kHz
        // Frame size = (144 * bitrate) / sample_rate
        let sample_rate = 44100; // Most common
        let frame_size = (144 * bitrate_kbps * 1000) / sample_rate;
        
        // Ensure minimum alignment and reasonable bounds
        frame_size.max(144).min(1728) // Between 144B and 1728B
    }
    
    // Detect actual ID3 tag size from file
    fn detect_id3_size(file_path: &std::path::Path) -> Option<u64> {
        use std::io::Read;
        
        let mut file = File::open(file_path).ok()?;
        let mut header = [0u8; 10];
        
        if file.read_exact(&mut header).is_err() {
            return Some(1024); // Default fallback
        }
        
        // Check for ID3v2 tag
        if &header[0..3] == b"ID3" {
            // Parse ID3v2 size (synchsafe integer)
            let size = ((header[6] as u32 & 0x7F) << 21) |
                      ((header[7] as u32 & 0x7F) << 14) |
                      ((header[8] as u32 & 0x7F) << 7) |
                      (header[9] as u32 & 0x7F);
            
            // Add header size (10 bytes) to tag size
            let total_size = (size + 10) as u64;
            
            info!("Detected ID3v2 tag size: {} bytes", total_size);
            Some(total_size)
        } else {
            // No ID3v2 tag, minimal offset
            Some(0)
        }
    }
    
    // Calculate more accurate bitrate
    fn calculate_accurate_bitrate(
        file_path: &std::path::Path,
        track: &crate::models::playlist::Track,
        file_size: u64
    ) -> u64 {
        // Try to read actual MP3 header for bitrate
        if let Some(actual_bitrate) = Self::read_mp3_bitrate(file_path) {
            info!("Using actual MP3 bitrate: {} kbps", actual_bitrate);
            return actual_bitrate;
        }
        
        // Fall back to file size calculation
        if track.duration > 0 && file_size > 0 {
            let calculated = (file_size * 8) / (track.duration * 1000);
            info!("Using calculated bitrate: {} kbps", calculated);
            calculated
        } else {
            info!("Using default bitrate: 128 kbps");
            128 // Default fallback
        }
    }
    
    // Read actual bitrate from MP3 header
    fn read_mp3_bitrate(file_path: &std::path::Path) -> Option<u64> {
        use std::io::Read;
        
        let mut file = File::open(file_path).ok()?;
        let mut buffer = [0u8; 4096]; // Read first 4KB to find MP3 frame
        
        if file.read(&mut buffer).is_err() {
            return None;
        }
        
        // Look for MP3 frame sync (11 bits set)
        for i in 0..buffer.len() - 4 {
            if buffer[i] == 0xFF && (buffer[i + 1] & 0xE0) == 0xE0 {
                // Found potential MP3 frame header
                let header = u32::from_be_bytes([
                    buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]
                ]);
                
                if let Some(bitrate) = Self::parse_mp3_header(header) {
                    return Some(bitrate);
                }
            }
        }
        
        None
    }
    
    // Parse MP3 frame header for bitrate
    fn parse_mp3_header(header: u32) -> Option<u64> {
        // MP3 bitrate table (kbps) for MPEG1 Layer 3
        const BITRATE_TABLE: [u16; 16] = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0
        ];
        
        // Extract bitrate index (bits 12-15)
        let bitrate_index = ((header >> 12) & 0xF) as usize;
        
        if bitrate_index < BITRATE_TABLE.len() && BITRATE_TABLE[bitrate_index] > 0 {
            Some(BITRATE_TABLE[bitrate_index] as u64)
        } else {
            None
        }
    }
    
    // Calculate optimal chunk size based on platform and bitrate
    fn calculate_optimal_chunk_size(platform: &Option<String>, bitrate_kbps: u64, max_content: u64) -> usize {
        let base_size = match platform.as_deref() {
            Some(p) if p.contains("android") || p.contains("Android") => {
                // Android needs smaller, more frequent chunks for better position accuracy
                config::MOBILE_INITIAL_BUFFER_SIZE * config::CHUNK_SIZE / 2
            },
            Some("ios") => config::IOS_INITIAL_BUFFER_SIZE * config::CHUNK_SIZE,
            Some("safari") => config::SAFARI_INITIAL_BUFFER_SIZE * config::CHUNK_SIZE,
            Some("mobile") => config::MOBILE_INITIAL_BUFFER_SIZE * config::CHUNK_SIZE,
            _ => config::DESKTOP_INITIAL_BUFFER_SIZE * config::CHUNK_SIZE,
        };
        
        // Adjust for bitrate
        let bitrate_multiplier = if bitrate_kbps > 192 { 1.5 } else { 1.0 };
        let adjusted_size = (base_size as f64 * bitrate_multiplier) as usize;
        
        // Ensure we don't exceed available content
        adjusted_size.min(max_content as usize).max(config::CHUNK_SIZE)
    }
    
    // Parse HTTP Range header for partial content requests
    fn parse_range_header(range: &str, file_size: u64) -> (u64, u64, bool) {
        if range.starts_with("bytes=") {
            let range_spec = &range[6..];
            
            if let Some(dash_pos) = range_spec.find('-') {
                let start_str = &range_spec[..dash_pos];
                let end_str = &range_spec[dash_pos + 1..];
                
                let start = start_str.parse::<u64>().unwrap_or(0);
                let end = if end_str.is_empty() {
                    file_size - 1 // Open-ended range (stream to end)
                } else {
                    end_str.parse::<u64>().unwrap_or(file_size - 1).min(file_size - 1)
                };
                
                if start < file_size {
                    info!("Range request: bytes {}-{}/{}", start, end, file_size);
                    return (start, end, true);
                }
            }
        }
        
        // Invalid range, serve from beginning
        (0, file_size - 1, false)
    }
}

impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, _request: &'r Request<'_>) -> response::Result<'static> {
        let mut response_builder = Response::build();
        
        // Set status
        response_builder.status(self.status);
        
        // Set content type
        response_builder.header(self.content_type);
        
        // Add all headers
        for header in self.headers {
            response_builder.header(header);
        }
        
        // Set body
        response_builder.sized_body(self.data.len(), std::io::Cursor::new(self.data));
        
        Ok(response_builder.finalize())
    }
}

// Range header guard to extract Range header from request
#[derive(Debug)]
pub struct RangeHeader(pub Option<String>);

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for RangeHeader {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        let range_header = request.headers().get_one("Range").map(|s| s.to_string());
        rocket::request::Outcome::Success(RangeHeader(range_header))
    }
}

// Main direct streaming endpoint
#[rocket::get("/direct-stream?<position>&<platform>")]
pub fn direct_stream(
    position: Option<u64>,
    platform: Option<String>,
    range_header: RangeHeader,
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStream, Status> {
    // Enhanced logging for Android debugging
    if let Some(ref platform_str) = platform {
        if platform_str.contains("android") || platform_str.contains("Android") {
            info!("Android direct stream request - position: {:?}, platform: {}", position, platform_str);
        } else {
            info!("Direct stream request from platform: {}", platform_str);
        }
    }
    
    if let Some(ref range) = range_header.0 {
        info!("Range request received: {}", range);
    }
    
    // Update listener count
    stream_manager.increment_listener_count();
    
    // Return the streaming handler
    DirectStream::new(
        stream_manager.inner().clone(), 
        position,
        platform,
        range_header.0
    )
}

// Stream status endpoint with enhanced info
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    let is_streaming = sm.is_streaming();
    let active_listeners = sm.get_active_listeners();
    let current_bitrate = sm.get_current_bitrate() / 1000; // Convert to kbps
    let (position_secs, position_ms) = sm.get_precise_position();
    
    // Get detailed track info
    let track_info = if let Some(track) = crate::services::playlist::get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "position_seconds": position_secs,
            "position_milliseconds": position_ms,
            "path": track.path
        })
    } else {
        serde_json::json!(null)
    };
    
    let status = serde_json::json!({
        "status": if is_streaming { "streaming" } else { "stopped" },
        "active_listeners": active_listeners,
        "stream_available": true,
        "playback_position": position_secs,
        "playback_position_ms": position_ms,
        "bitrate_kbps": current_bitrate,
        "current_track": track_info,
        "server_time": chrono::Local::now().to_rfc3339(),
        "supports_range_requests": true,
        "chunked_streaming": true,
        "ios_optimized": true,
        "android_optimized": true,
        "streaming_method": "position_sync_chunked_android_fixed",
        "position_accuracy": "millisecond",
        "android_fixes": {
            "server_authoritative_position": true,
            "strict_position_validation": true,
            "enhanced_debug_headers": true
        }
    });
    
    rocket::serde::json::Json(status)
}

// CORS preflight handler
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
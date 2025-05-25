// src/direct_stream.rs - Fixed version with proper mobile connection management

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use log::{info, error, warn, debug};

use crate::services::streamer::StreamManager;
use crate::services::playlist;
use crate::config;

// Mobile-optimized streaming responder
pub struct DirectStream {
    data: Vec<u8>,
    content_type: ContentType,
    headers: Vec<Header<'static>>,
    status: Status,
    connection_id: String, // Track connection for proper cleanup
}

impl DirectStream {
    pub fn new(
        stream_manager: Arc<StreamManager>, 
        requested_position: Option<u64>,
        platform: Option<String>,
        range_header: Option<String>,
        ios_optimized: Option<bool>,
        chunk_size: Option<usize>,
        initial_buffer: Option<usize>
    ) -> Result<Self, Status> {
        // Get connection ID for tracking
        let connection_id = stream_manager.increment_listener_count();
        
        // Get the current track from the playlist
        let track = match playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            Some(track) => track,
            None => {
                error!("No track available to stream");
                stream_manager.decrement_listener_count(&connection_id);
                return Err(Status::NotFound);
            }
        };
        
        // Enhanced platform detection
        let platform_info = Self::detect_platform(&platform);
        
        info!("Mobile streaming request: {} by {} (platform: {:?}, connection: {})", 
              track.title, track.artist, platform_info, &connection_id[..8]);
        
        // Get server position with mobile-friendly synchronization
        let (server_position_secs, server_position_ms) = stream_manager.get_precise_position();
        
        // Radio-style streaming: Always use current server position
        // This ensures all clients hear the same thing at the same time, like a real radio station
        let sync_position = server_position_secs;
        
        if let Some(req_pos) = requested_position {
            let diff = (req_pos as i64 - server_position_secs as i64).abs();
            info!("Radio mode: Client requested {}s, but serving current radio position {}s (diff: {}s)", 
                  req_pos, server_position_secs, diff);
        } else {
            info!("Radio mode: Serving current radio position: {}s", server_position_secs);
        }
        
        // Ensure we never serve from position 0 unless the track just started
        let actual_sync_position = if sync_position == 0 && server_position_ms < 1000 {
            // Track just started, it's okay to serve from beginning
            0
        } else if sync_position == 0 {
            // This shouldn't happen - track manager might not be running properly
            warn!("Radio mode: Server position is 0 but track should be playing. Using 1s offset.");
            1
        } else {
            sync_position
        };
        
        info!("Radio mode: Final sync position: {}s", actual_sync_position);
        
        // Create the full path to the MP3 file
        let file_path = config::MUSIC_FOLDER.join(&track.path);
        
        if !file_path.exists() {
            error!("Track file not found: {}", file_path.display());
            stream_manager.decrement_listener_count(&connection_id);
            return Err(Status::NotFound);
        }
        
        // Get file metadata
        let file_metadata = match std::fs::metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(e) => {
                error!("Error getting file metadata: {}", e);
                stream_manager.decrement_listener_count(&connection_id);
                return Err(Status::InternalServerError);
            }
        };
        
        let file_size = file_metadata.len();
        let bitrate_kbps = Self::calculate_accurate_bitrate(&file_path, &track, file_size);
        
        info!("Mobile streaming: pos={}s, bitrate={}kbps, size={}B, device={}", 
              sync_position,
            actual_sync_position, bitrate_kbps, file_size, platform_info.device_type);
        
        // Determine start and end bytes based on range header or position
        let (start_byte, end_byte, is_partial) = if let Some(range) = &range_header {
            Self::parse_range_header(range, file_size)
        } else {
            let byte_position = Self::time_to_bytes_mobile_optimized(
                actual_sync_position, 
                bitrate_kbps, 
                file_size,
                &file_path,
                &platform_info
            );
            
            let start = if byte_position >= file_size { 
                warn!("Calculated position exceeds file size, starting from beginning");
                0 
            } else { 
                byte_position 
            };
            
            debug!("Mobile position sync: {}s -> byte {} of {}", actual_sync_position, start, file_size);
            (start, file_size - 1, false)
        };
        
        // Determine chunk size based on platform and iOS optimizations
        let _content_length = end_byte - start_byte + 1;
        let ios_params = if ios_optimized.unwrap_or(false) {
            Some((chunk_size.unwrap_or(32768), initial_buffer.unwrap_or(65536)))
        } else {
            None
        };
        
        let data = match Self::read_file_chunk(&file_path, start_byte, end_byte, &platform_info, ios_params) {
            Ok(data) => data,
            Err(e) => {
                error!("Error reading file: {}", e);
                stream_manager.decrement_listener_count(&connection_id);
                return Err(Status::InternalServerError);
            }
        };
        
        // Build mobile-optimized headers
        let headers = Self::build_mobile_headers(
            &platform_info, 
            &track, 
            data.len(),
            start_byte,
            file_size,
            is_partial,
            bitrate_kbps,
            sync_position,
            server_position_secs,
            server_position_ms,
            &connection_id
        );
        
        let status = if is_partial { Status::PartialContent } else { Status::Ok };
        
        Ok(DirectStream {
            data,
            content_type: ContentType::new("audio", "mpeg"),
            headers,
            status,
            connection_id,
        })
    }
    
    fn detect_platform(platform: &Option<String>) -> PlatformInfo {
        let platform_str = platform.as_deref().unwrap_or("");
        
        PlatformInfo {
            device_type: if platform_str.contains("android") || platform_str.contains("Android") {
                "android".to_string()
            } else if platform_str == "ios" || platform_str == "safari" {
                "ios".to_string()
            } else if platform_str == "mobile" {
                "mobile".to_string()
            } else {
                "desktop".to_string()
            },
            user_agent: platform_str.to_string(),
            is_mobile: platform_str.contains("android") || 
                      platform_str.contains("Android") ||
                      platform_str == "ios" ||
                      platform_str == "mobile",
        }
    }
    
    fn time_to_bytes_mobile_optimized(
        position_seconds: u64, 
        bitrate_kbps: u64, 
        file_size: u64,
        file_path: &std::path::Path,
        platform_info: &PlatformInfo
    ) -> u64 {
        if bitrate_kbps == 0 || position_seconds == 0 {
            return 0;
        }
        
        // Detect ID3 tag size
        let id3_offset = Self::detect_id3_size(file_path).unwrap_or(0);
        
        // Convert time to bytes
        let bytes_per_second = (bitrate_kbps * 1000) / 8;
        let byte_position = id3_offset + (position_seconds * bytes_per_second);
        
        // Mobile-optimized frame alignment (less strict for better compatibility)
        let frame_size = if platform_info.is_mobile {
            // Use larger alignment for mobile to reduce overhead
            1024 // 1KB alignment for mobile devices
        } else {
            // Desktop can handle more precise alignment
            Self::calculate_mp3_frame_size(bitrate_kbps)
        };
        
        let aligned_position = (byte_position / frame_size) * frame_size;
        let max_position = file_size.saturating_sub(config::CHUNK_SIZE as u64);
        
        let final_position = aligned_position.min(max_position);
        
        debug!("Mobile position calc: {}s -> {}B (ID3: {}B, align: {}B)", 
               position_seconds, final_position, id3_offset, frame_size);
        
        final_position
    }
    
    fn calculate_mp3_frame_size(bitrate_kbps: u64) -> u64 {
        // Standard MP3 frame calculation for 44.1kHz
        let sample_rate = 44100;
        let frame_size = (144 * bitrate_kbps * 1000) / sample_rate;
        frame_size.max(144).min(1728)
    }
    
    fn detect_id3_size(file_path: &std::path::Path) -> Option<u64> {
        use std::io::Read;
        
        let mut file = File::open(file_path).ok()?;
        let mut header = [0u8; 10];
        
        if file.read_exact(&mut header).is_err() {
            return Some(0);
        }
        
        if &header[0..3] == b"ID3" {
            let size = ((header[6] as u32 & 0x7F) << 21) |
                      ((header[7] as u32 & 0x7F) << 14) |
                      ((header[8] as u32 & 0x7F) << 7) |
                      (header[9] as u32 & 0x7F);
            
            let total_size = (size + 10) as u64;
            debug!("Detected ID3v2 tag size: {} bytes", total_size);
            Some(total_size)
        } else {
            Some(0)
        }
    }
    
    fn calculate_accurate_bitrate(
        file_path: &std::path::Path,
        track: &crate::models::playlist::Track,
        file_size: u64
    ) -> u64 {
        // Try to read actual MP3 header for bitrate
        if let Some(actual_bitrate) = Self::read_mp3_bitrate(file_path) {
            debug!("Using actual MP3 bitrate: {} kbps", actual_bitrate);
            return actual_bitrate;
        }
        
        // Fall back to file size calculation
        if track.duration > 0 && file_size > 0 {
            let calculated = (file_size * 8) / (track.duration * 1000);
            debug!("Using calculated bitrate: {} kbps", calculated);
            calculated
        } else {
            debug!("Using default bitrate: 128 kbps");
            128
        }
    }
    
    fn read_mp3_bitrate(file_path: &std::path::Path) -> Option<u64> {
        use std::io::Read;
        
        let mut file = File::open(file_path).ok()?;
        let mut buffer = [0u8; 4096];
        
        if file.read(&mut buffer).is_err() {
            return None;
        }
        
        // Look for MP3 frame sync
        for i in 0..buffer.len() - 4 {
            if buffer[i] == 0xFF && (buffer[i + 1] & 0xE0) == 0xE0 {
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
    
    fn parse_mp3_header(header: u32) -> Option<u64> {
        const BITRATE_TABLE: [u16; 16] = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0
        ];
        
        let bitrate_index = ((header >> 12) & 0xF) as usize;
        
        if bitrate_index < BITRATE_TABLE.len() && BITRATE_TABLE[bitrate_index] > 0 {
            Some(BITRATE_TABLE[bitrate_index] as u64)
        } else {
            None
        }
    }
    
    fn read_file_chunk(
        file_path: &std::path::Path,
        start_byte: u64,
        end_byte: u64,
        platform_info: &PlatformInfo,
        ios_params: Option<(usize, usize)>
    ) -> Result<Vec<u8>, std::io::Error> {
        let mut file = File::open(file_path)?;
        
        // Seek to start position
        if start_byte > 0 {
            file.seek(SeekFrom::Start(start_byte))?;
        }
        
        // Calculate chunk size based on platform and iOS params
        let content_length = end_byte - start_byte + 1;
        let chunk_size = if let Some((ios_chunk_size, _)) = ios_params {
            ios_chunk_size
        } else {
            Self::calculate_mobile_chunk_size(platform_info, content_length)
        };
        
        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = file.read(&mut buffer)?;
        
        // Truncate buffer to actual bytes read
        buffer.truncate(bytes_read);
        
        debug!("Read {} bytes for {} device", bytes_read, platform_info.device_type);
        Ok(buffer)
    }
    
    fn calculate_mobile_chunk_size(platform_info: &PlatformInfo, max_content: u64) -> usize {
        let base_size = match platform_info.device_type.as_str() {
            "android" => config::CHUNK_SIZE * 8,  // 128KB for Android (good balance)
            "ios" => config::CHUNK_SIZE * 6,      // 96KB for iOS (Safari optimization) 
            "mobile" => config::CHUNK_SIZE * 10,  // 160KB for other mobile
            _ => config::CHUNK_SIZE * 12,         // 192KB for desktop
        };
        
        // Ensure we don't exceed available content
        base_size.min(max_content as usize).max(config::CHUNK_SIZE)
    }
    
    fn build_mobile_headers(
        platform_info: &PlatformInfo,
        track: &crate::models::playlist::Track,
        content_length: usize,
        start_byte: u64,
        file_size: u64,
        is_partial: bool,
        bitrate_kbps: u64,
        sync_position: u64,
        actual_sync_position: u64,
        server_position_secs: u64,
        server_position_ms: u64,
        connection_id: &str
    ) -> Vec<Header<'static>> {
        let mut headers = Vec::new();
        
        // Essential headers for audio streaming
        headers.push(Header::new("Accept-Ranges", "bytes"));
        headers.push(Header::new("Content-Length", content_length.to_string()));
        
        // Mobile-optimized caching and connection headers
        match platform_info.device_type.as_str() {
            "android" => {
                // Android-specific optimizations
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=30, max=1000"));
                headers.push(Header::new("X-Android-Optimized", "true"));
            },
            "ios" => {
                // iOS/Safari needs different settings for radio streaming
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=120, max=100"));
                headers.push(Header::new("X-iOS-Radio-Optimized", "true"));
                // Disable iOS buffering that causes issues with live streams
                headers.push(Header::new("X-Content-Duration", "LIVE"));
                headers.push(Header::new("X-Playback-Session-Id", connection_id[..8].to_string()));
            },
            "mobile" => {
                // General mobile optimization
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=45, max=1000"));
                headers.push(Header::new("X-Mobile-Optimized", "true"));
            },
            _ => {
                // Desktop browsers
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=120, max=1000"));
            }
        }
        
        // CORS headers for web player compatibility
        headers.push(Header::new("Access-Control-Allow-Origin", "*"));
        headers.push(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"));
        headers.push(Header::new("Access-Control-Allow-Headers", "Range"));
        headers.push(Header::new("X-Content-Type-Options", "nosniff"));
        
        // Enhanced debug headers for mobile troubleshooting
        headers.push(Header::new("X-Track-Title", track.title.clone()));
        headers.push(Header::new("X-Track-Duration", track.duration.to_string()));
        headers.push(Header::new("X-Bitrate-Kbps", bitrate_kbps.to_string()));
        headers.push(Header::new("X-Start-Byte", start_byte.to_string()));
        headers.push(Header::new("X-Content-Length", content_length.to_string()));
        headers.push(Header::new("X-Streaming-Method", "mobile_optimized_chunked"));
        headers.push(Header::new("X-Server-Position", server_position_secs.to_string()));
        headers.push(Header::new("X-Server-Position-Ms", server_position_ms.to_string()));
        headers.push(Header::new("X-Position-Used", actual_sync_position.to_string()));
        headers.push(Header::new("X-Position-Requested", sync_position.to_string()));
        headers.push(Header::new("X-Connection-ID", connection_id[..8].to_string()));
        headers.push(Header::new("X-Platform", platform_info.device_type.clone()));
        
        // Mobile-specific debugging headers
        if platform_info.is_mobile {
            headers.push(Header::new("X-Mobile-Debug", "connection_managed"));
            headers.push(Header::new("X-Mobile-Chunk-Size", 
                Self::calculate_mobile_chunk_size(platform_info, file_size).to_string()));
        }
        
        // Partial content header if needed
        if is_partial {
            headers.push(Header::new("Content-Range", 
                format!("bytes {}-{}/{}", start_byte, start_byte + content_length as u64 - 1, file_size)));
        }
        
        headers
    }
    
    fn parse_range_header(range: &str, file_size: u64) -> (u64, u64, bool) {
        if range.starts_with("bytes=") {
            let range_spec = &range[6..];
            
            if let Some(dash_pos) = range_spec.find('-') {
                let start_str = &range_spec[..dash_pos];
                let end_str = &range_spec[dash_pos + 1..];
                
                let start = start_str.parse::<u64>().unwrap_or(0);
                let end = if end_str.is_empty() {
                    file_size - 1
                } else {
                    end_str.parse::<u64>().unwrap_or(file_size - 1).min(file_size - 1)
                };
                
                if start < file_size {
                    info!("Range request: bytes {}-{}/{}", start, end, file_size);
                    return (start, end, true);
                }
            }
        }
        
        (0, file_size - 1, false)
    }
}

#[derive(Debug)]
struct PlatformInfo {
    device_type: String,
    user_agent: String,
    is_mobile: bool,
}

impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, _request: &'r Request<'_>) -> response::Result<'static> {
        let mut response_builder = Response::build();
        
        response_builder.status(self.status);
        response_builder.header(self.content_type);
        
        for header in self.headers {
            response_builder.header(header);
        }
        
        response_builder.sized_body(self.data.len(), std::io::Cursor::new(self.data));
        
        Ok(response_builder.finalize())
    }
}

// Connection tracking for proper listener management
#[derive(Debug)]
pub struct ConnectionTracker {
    pub id: String,
}

impl Drop for ConnectionTracker {
    fn drop(&mut self) {
        // This would ideally notify the StreamManager, but we need a reference
        // In practice, the cleanup happens via periodic cleanup in StreamManager
        debug!("Connection {} dropped", &self.id[..8]);
    }
}

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

// Main direct streaming endpoint with iOS optimizations
#[rocket::get("/direct-stream?<position>&<platform>&<ios_optimized>&<chunk_size>&<initial_buffer>&<min_buffer_time>&<preload>&<buffer_recovery>")]
pub fn direct_stream(
    position: Option<u64>,
    platform: Option<String>,
    ios_optimized: Option<bool>,
    chunk_size: Option<usize>,
    initial_buffer: Option<usize>,
    min_buffer_time: Option<u64>,
    preload: Option<String>,
    buffer_recovery: Option<u64>,
    range_header: RangeHeader,
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStream, Status> {
    // Enhanced logging for mobile debugging
    let platform_str = platform.as_deref().unwrap_or("unknown");
    let is_mobile = platform_str.contains("android") || 
                   platform_str.contains("Android") ||
                   platform_str == "ios" ||
                   platform_str == "mobile";
    
    if is_mobile {
        info!("Mobile direct stream request - platform: {}, position: {:?}", platform_str, position);
    } else {
        debug!("Desktop direct stream request - platform: {}, position: {:?}", platform_str, position);
    }
    
    if let Some(ref range) = range_header.0 {
        debug!("Range request received: {}", range);
    }
    
    // Log iOS-specific parameters if provided
    if ios_optimized.unwrap_or(false) {
        debug!("iOS optimizations requested - chunk_size: {:?}, initial_buffer: {:?}, min_buffer_time: {:?}", 
               chunk_size, initial_buffer, min_buffer_time);
    }
    
    if preload.is_some() {
        debug!("Preload setting: {:?}", preload);
    }
    
    if buffer_recovery.is_some() {
        debug!("Buffer recovery mode: {:?}", buffer_recovery);
    }
    
    // Cleanup stale connections periodically
    stream_manager.cleanup_stale_connections();
    
    // Return the streaming handler (connection tracking happens inside DirectStream::new)
    DirectStream::new(
        stream_manager.inner().clone(), 
        position,
        platform,
        range_header.0,
        ios_optimized,
        chunk_size,
        initial_buffer
    )
}

// Stream status endpoint with accurate listener count
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    
    // Clean up stale connections before reporting count
    sm.cleanup_stale_connections();
    
    let is_streaming = sm.is_streaming();
    let active_listeners = sm.get_active_listeners(); // This now returns accurate count
    let current_bitrate = sm.get_current_bitrate() / 1000;
    let (position_secs, position_ms) = sm.get_precise_position();
    
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
        "mobile_optimized": true,
        "streaming_method": "mobile_optimized_chunked",
        "position_accuracy": "millisecond",
        "connection_management": "tracked",
        "mobile_features": {
            "android_optimized": true,
            "ios_optimized": true,
            "connection_tracking": true,
            "stale_cleanup": true,
            "accurate_listener_count": true
        }
    });
    
    rocket::serde::json::Json(status)
}

// CORS preflight handler
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
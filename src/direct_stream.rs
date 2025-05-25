// src/direct_stream.rs - Fixed for true radio mode and iOS streaming

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

// Radio streaming responder
pub struct DirectStream {
    data: Vec<u8>,
    content_type: ContentType,
    headers: Vec<Header<'static>>,
    status: Status,
    connection_id: String,
}

impl DirectStream {
    pub fn new(
        stream_manager: Arc<StreamManager>, 
        _requested_position: Option<u64>, // Ignored in radio mode
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
        
        info!("RADIO MODE: New listener connecting - {} (connection: {})", 
              platform_info.device_type, &connection_id[..8]);
        
        // RADIO MODE: Always get current server position (ignore any client request)
        let (server_position_secs, server_position_ms) = stream_manager.get_precise_position();
        
        info!("RADIO MODE: Current server radio time: {}s + {}ms", 
              server_position_secs, server_position_ms);
        
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
        
        info!("RADIO MODE: Serving from position {}s, bitrate={}kbps, file={}B, platform={}", 
              server_position_secs, bitrate_kbps, file_size, platform_info.device_type);
        
        // Handle range requests or calculate position-based streaming
        let (start_byte, end_byte, is_partial) = if let Some(range) = &range_header {
            Self::parse_range_header(range, file_size)
        } else {
            // RADIO MODE: Calculate byte position from current server time
            let byte_position = Self::calculate_radio_byte_position(
                server_position_secs, 
                bitrate_kbps, 
                file_size,
                &file_path,
                &platform_info
            );
            
            info!("RADIO MODE: Calculated byte position {} for time {}s", 
                  byte_position, server_position_secs);
            
            // For iOS, we need to stream from current position to end
            // For other platforms, we can also stream from current position
            (byte_position, file_size - 1, false)
        };
        
        // Special handling for iOS to prevent fragmentation
        let data = if platform_info.device_type == "ios" {
            Self::read_ios_radio_chunk(&file_path, start_byte, file_size, &connection_id)?
        } else {
            Self::read_standard_radio_chunk(&file_path, start_byte, end_byte, &platform_info)?
        };
        
        // Build radio-optimized headers
        let headers = Self::build_radio_headers(
            &platform_info, 
            &track, 
            data.len(),
            start_byte,
            file_size,
            is_partial,
            bitrate_kbps,
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
            device_type: if platform_str == "ios" || platform_str.contains("safari") {
                "ios".to_string()
            } else if platform_str.contains("android") || platform_str.contains("Android") {
                "android".to_string()
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
    
    // Calculate byte position for radio streaming
    fn calculate_radio_byte_position(
        position_seconds: u64, 
        bitrate_kbps: u64, 
        file_size: u64,
        file_path: &std::path::Path,
        platform_info: &PlatformInfo
    ) -> u64 {
        if bitrate_kbps == 0 || position_seconds == 0 {
            return 0;
        }
        
        // Detect ID3 tag size to skip metadata
        let id3_offset = Self::detect_id3_size(file_path).unwrap_or(0);
        
        // Convert time to bytes
        let bytes_per_second = (bitrate_kbps * 1000) / 8;
        let raw_byte_position = id3_offset + (position_seconds * bytes_per_second);
        
        // Ensure we don't exceed file size
        let max_position = file_size.saturating_sub(1024); // Leave 1KB buffer at end
        let clamped_position = raw_byte_position.min(max_position);
        
        // For iOS, align to larger boundaries to prevent fragmentation
        let alignment = if platform_info.device_type == "ios" {
            2048 // 2KB alignment for iOS
        } else {
            1024 // 1KB alignment for others
        };
        
        let aligned_position = (clamped_position / alignment) * alignment;
        
        debug!("RADIO POSITION: {}s -> raw={}B, clamped={}B, aligned={}B (ID3={}B)", 
               position_seconds, raw_byte_position, clamped_position, aligned_position, id3_offset);
        
        aligned_position
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
    
    // Special iOS radio chunk reading to prevent fragmentation
    fn read_ios_radio_chunk(
        file_path: &std::path::Path,
        start_byte: u64,
        file_size: u64,
        connection_id: &str
    ) -> Result<Vec<u8>, Status> {
        let mut file = File::open(file_path).map_err(|_| Status::InternalServerError)?;
        
        // Seek to start position
        if start_byte > 0 {
            file.seek(SeekFrom::Start(start_byte)).map_err(|_| Status::InternalServerError)?;
        }
        
        // For iOS radio, we read a larger continuous chunk to prevent buffering issues
        // This gives iOS a substantial buffer to work with
        let remaining_bytes = file_size - start_byte;
        let chunk_size = std::cmp::min(remaining_bytes, 256 * 1024) as usize; // 256KB for iOS radio
        
        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = file.read(&mut buffer).map_err(|_| Status::InternalServerError)?;
        
        // Truncate buffer to actual bytes read
        buffer.truncate(bytes_read);
        
        info!("iOS RADIO: Read {}KB continuous chunk for connection {}", 
              bytes_read / 1024, &connection_id[..8]);
        
        Ok(buffer)
    }
    
    // Standard radio chunk reading for other platforms
    fn read_standard_radio_chunk(
        file_path: &std::path::Path,
        start_byte: u64,
        end_byte: u64,
        platform_info: &PlatformInfo
    ) -> Result<Vec<u8>, Status> {
        let mut file = File::open(file_path).map_err(|_| Status::InternalServerError)?;
        
        // Seek to start position
        if start_byte > 0 {
            file.seek(SeekFrom::Start(start_byte)).map_err(|_| Status::InternalServerError)?;
        }
        
        // Calculate appropriate chunk size for radio streaming
        let content_length = end_byte - start_byte + 1;
        let chunk_size = match platform_info.device_type.as_str() {
            "android" => std::cmp::min(content_length, 128 * 1024), // 128KB for Android
            "mobile" => std::cmp::min(content_length, 96 * 1024),   // 96KB for mobile
            _ => std::cmp::min(content_length, 192 * 1024),         // 192KB for desktop
        } as usize;
        
        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = file.read(&mut buffer).map_err(|_| Status::InternalServerError)?;
        
        // Truncate buffer to actual bytes read
        buffer.truncate(bytes_read);
        
        debug!("RADIO: Read {}KB chunk for {} platform", 
               bytes_read / 1024, platform_info.device_type);
        
        Ok(buffer)
    }
    
    fn build_radio_headers(
        platform_info: &PlatformInfo,
        track: &crate::models::playlist::Track,
        content_length: usize,
        start_byte: u64,
        file_size: u64,
        is_partial: bool,
        bitrate_kbps: u64,
        server_position_secs: u64,
        server_position_ms: u64,
        connection_id: &str
    ) -> Vec<Header<'static>> {
        let mut headers = Vec::new();
        
        // Essential headers for radio streaming
        headers.push(Header::new("Accept-Ranges", "bytes"));
        headers.push(Header::new("Content-Length", content_length.to_string()));
        
        // Radio-specific caching and connection headers
        match platform_info.device_type.as_str() {
            "ios" => {
                // iOS radio streaming - prevent aggressive buffering
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "close")); // Close connection for iOS to prevent issues
                headers.push(Header::new("X-iOS-Radio-Stream", "continuous"));
                headers.push(Header::new("X-Content-Duration", "LIVE"));
                headers.push(Header::new("X-iOS-Buffer-Strategy", "large-chunk"));
            },
            "android" => {
                // Android radio streaming
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=30, max=100"));
                headers.push(Header::new("X-Android-Radio-Stream", "optimized"));
            },
            "mobile" => {
                // General mobile radio
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Pragma", "no-cache"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=45, max=100"));
                headers.push(Header::new("X-Mobile-Radio-Stream", "standard"));
            },
            _ => {
                // Desktop radio streaming
                headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
                headers.push(Header::new("Connection", "keep-alive"));
                headers.push(Header::new("Keep-Alive", "timeout=120, max=200"));
                headers.push(Header::new("X-Desktop-Radio-Stream", "optimized"));
            }
        }
        
        // CORS headers
        headers.push(Header::new("Access-Control-Allow-Origin", "*"));
        headers.push(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"));
        headers.push(Header::new("Access-Control-Allow-Headers", "Range"));
        headers.push(Header::new("X-Content-Type-Options", "nosniff"));
        
        // Radio synchronization headers - critical for position sync
        headers.push(Header::new("X-Radio-Mode", "synchronized"));
        headers.push(Header::new("X-Radio-Position-Seconds", server_position_secs.to_string()));
        headers.push(Header::new("X-Radio-Position-Milliseconds", server_position_ms.to_string()));
        headers.push(Header::new("X-Radio-Byte-Start", start_byte.to_string()));
        headers.push(Header::new("X-Radio-Sync-Timestamp", 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string()));
        
        // Track and streaming info
        headers.push(Header::new("X-Track-Title", track.title.clone()));
        headers.push(Header::new("X-Track-Duration", track.duration.to_string()));
        headers.push(Header::new("X-Bitrate-Kbps", bitrate_kbps.to_string()));
        headers.push(Header::new("X-Content-Length", content_length.to_string()));
        headers.push(Header::new("X-Streaming-Method", "radio_synchronized"));
        headers.push(Header::new("X-Connection-ID", connection_id[..8].to_string()));
        headers.push(Header::new("X-Platform", platform_info.device_type.clone()));
        
        // Platform-specific debugging
        if platform_info.device_type == "ios" {
            headers.push(Header::new("X-iOS-Chunk-Size", format!("{}KB", content_length / 1024)));
            headers.push(Header::new("X-iOS-Strategy", "large-continuous-chunk"));
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

// Main radio streaming endpoint
#[rocket::get("/direct-stream?<position>&<platform>&<ios_optimized>&<chunk_size>&<initial_buffer>&<min_buffer_time>&<preload>&<buffer_recovery>")]
pub fn direct_stream(
    position: Option<u64>,        // Ignored in radio mode
    platform: Option<String>,
    ios_optimized: Option<bool>,  // Ignored - iOS gets special handling automatically
    chunk_size: Option<usize>,    // Ignored - calculated per platform
    initial_buffer: Option<usize>, // Ignored - calculated per platform
    min_buffer_time: Option<u64>, // Ignored in radio mode
    preload: Option<String>,      // Ignored in radio mode
    buffer_recovery: Option<u64>, // Ignored in radio mode
    range_header: RangeHeader,
    stream_manager: &State<Arc<StreamManager>>
) -> Result<DirectStream, Status> {
    let platform_str = platform.as_deref().unwrap_or("unknown");
    
    info!("RADIO STREAM REQUEST: platform={}, ignoring position parameter (radio mode)", platform_str);
    
    if let Some(ref range) = range_header.0 {
        debug!("Range request received: {}", range);
    }
    
    // Cleanup stale connections
    stream_manager.cleanup_stale_connections();
    
    // Return radio streaming (position is ignored, server determines current time)
    DirectStream::new(
        stream_manager.inner().clone(), 
        None, // Always None in radio mode
        platform,
        range_header.0,
        None, // iOS optimization handled automatically
        None, // Chunk size calculated per platform
        None  // Buffer size calculated per platform
    )
}

// Stream status endpoint
#[rocket::get("/stream-status")]
pub fn stream_status(stream_manager: &State<Arc<StreamManager>>) -> rocket::serde::json::Json<serde_json::Value> {
    let sm = stream_manager.inner();
    
    // Clean up stale connections before reporting count
    sm.cleanup_stale_connections();
    
    let is_streaming = sm.is_streaming();
    let active_listeners = sm.get_active_listeners();
    let current_bitrate = sm.get_current_bitrate() / 1000;
    let (position_secs, position_ms) = sm.get_precise_position();
    
    let track_info = if let Some(track) = crate::services::playlist::get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        serde_json::json!({
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration,
            "radio_position": position_secs,
            "radio_position_ms": position_ms,
            "path": track.path
        })
    } else {
        serde_json::json!(null)
    };
    
    let status = serde_json::json!({
        "status": if is_streaming { "streaming" } else { "stopped" },
        "active_listeners": active_listeners,
        "stream_available": true,
        "radio_position": position_secs,
        "radio_position_ms": position_ms,
        "bitrate_kbps": current_bitrate,
        "current_track": track_info,
        "server_time": chrono::Local::now().to_rfc3339(),
        "supports_range_requests": true,
        "streaming_method": "radio_synchronized",
        "streaming_mode": "radio",
        "seeking_enabled": false,
        "synchronized_playback": true,
        "radio_features": {
            "position_ignored": true,
            "server_time_authoritative": true,
            "ios_large_chunks": true,
            "android_optimized": true,
            "desktop_optimized": true
        }
    });
    
    rocket::serde::json::Json(status)
}

// CORS preflight handler
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
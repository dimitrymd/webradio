// src/direct_stream.rs - Final fixed version

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use log::{info, error};

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
        
        info!("Chunked streaming: {} by {} (platform: {:?})", 
              track.title, track.artist, platform);
        
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
        
        // Calculate bitrate from file size and track duration
        let bitrate_kbps = if track.duration > 0 && file_size > 0 {
            (file_size * 8) / (track.duration * 1000)
        } else {
            128 // Default 128kbps
        };
        
        // Determine start and end bytes based on range header or position
        let (start_byte, end_byte, is_partial) = if let Some(range) = &range_header {
            Self::parse_range_header(range, file_size)
        } else {
            // Always use current server position for synchronized playback
            let server_position = requested_position.unwrap_or_else(|| stream_manager.get_playback_position());
            let byte_position = Self::time_to_bytes(server_position, bitrate_kbps);
            let start = if byte_position >= file_size { 
                // If calculated position is beyond file, start from beginning
                info!("Calculated position {}s (byte {}) exceeds file size, starting from beginning", server_position, byte_position);
                0 
            } else { 
                byte_position 
            };
            
            info!("Position-synchronized streaming: {}s -> byte {} of {} (bitrate: {}kbps)", 
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
        
        // Read the requested chunk (limit to reasonable size for chunked streaming)
        let chunk_size = (content_length as usize).min(config::CHUNK_SIZE * 8); // Max 8 chunks at once
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
        let is_ios = platform.as_deref() == Some("ios") || platform.as_deref() == Some("safari");
        if is_ios {
            // iOS prefers some caching for smoother playback
            headers.push(Header::new("Cache-Control", "public, max-age=3600"));
            headers.push(Header::new("Connection", "keep-alive"));
        } else {
            // Other browsers - no caching for live streaming feel
            headers.push(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"));
            headers.push(Header::new("Pragma", "no-cache"));
        }
        
        // CORS headers for web player compatibility
        headers.push(Header::new("Access-Control-Allow-Origin", "*"));
        headers.push(Header::new("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"));
        headers.push(Header::new("Access-Control-Allow-Headers", "Range"));
        headers.push(Header::new("X-Content-Type-Options", "nosniff"));
        
        // Debug headers for troubleshooting
        headers.push(Header::new("X-Track-Title", track.title.clone()));
        headers.push(Header::new("X-Track-Duration", track.duration.to_string()));
        headers.push(Header::new("X-Bitrate-Kbps", bitrate_kbps.to_string()));
        headers.push(Header::new("X-Start-Byte", start_byte.to_string()));
        headers.push(Header::new("X-Content-Length", bytes_read.to_string()));
        headers.push(Header::new("X-Streaming-Method", "chunked".to_string()));
        
        if is_ios {
            headers.push(Header::new("X-iOS-Optimized", "true"));
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
    
    // Calculate byte position from time position
    fn time_to_bytes(position_seconds: u64, bitrate_kbps: u64) -> u64 {
        if bitrate_kbps == 0 {
            return 0;
        }
        
        // Conservative ID3 offset for MP3 files
        const ID3_OFFSET: u64 = 8192;
        
        // Convert time to bytes: bitrate (kbps) * 1000 / 8 = bytes per second
        let bytes_per_second = (bitrate_kbps * 1000) / 8;
        let byte_position = ID3_OFFSET + (position_seconds * bytes_per_second);
        
        // Round to 1KB boundary for better streaming compatibility
        if position_seconds > 0 {
            (byte_position / 1024) * 1024
        } else {
            0
        }
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
    // Log the request for debugging
    if let Some(ref platform_str) = platform {
        info!("Direct stream request from platform: {}", platform_str);
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

// Stream status endpoint with chunked streaming info
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
            "position": playback_position,
            "path": track.path
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
        "server_time": chrono::Local::now().to_rfc3339(),
        "supports_range_requests": true,
        "chunked_streaming": true,
        "ios_optimized": true,
        "streaming_method": "direct_chunked"
    });
    
    rocket::serde::json::Json(status)
}

// CORS preflight handler
#[rocket::options("/direct-stream")]
pub fn direct_stream_options() -> rocket::response::status::NoContent {
    rocket::response::status::NoContent
}
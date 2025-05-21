// direct_stream.rs - Position-synchronized direct streaming implementation

use rocket::http::{ContentType, Header, Status};
use rocket::response::{self, Responder, Response};
use rocket::State;
use rocket::Request;
use std::sync::Arc;
use std::path::Path;
use std::fs::File;  // Standard File
use std::io::{self, Read, Seek, SeekFrom};  // For reading operations
use log::{info, error};

use crate::services::streamer::StreamManager;
use crate::services::playlist;
use crate::config;

pub struct DirectStream {
    stream_manager: Arc<StreamManager>,
    requested_position: Option<u64>,
}

impl DirectStream {
    pub fn new(stream_manager: Arc<StreamManager>, requested_position: Option<u64>) -> Self {
        Self { 
            stream_manager,
            requested_position,
        }
    }
    
    // Calculate byte position from time position
    fn time_to_bytes(position_seconds: u64, bitrate_kbps: u64) -> u64 {
        // For MP3, we can roughly estimate byte position using bitrate
        // Formula: bytes = seconds * (bitrate * 1000 / 8)
        if bitrate_kbps == 0 {
            return 0; // Avoid division by zero
        }
        
        // Convert time to bytes (bitrate is in kbps, so multiply by 1000/8 to get bytes/sec)
        // Add a small offset for ID3 tags
        const ID3_OFFSET: u64 = 4000; // Approximate size for ID3 tags
        ID3_OFFSET + (position_seconds * (bitrate_kbps * 125))
    }
}

// Use a simple file response - this will use Rocket's built-in file serving capability
impl<'r> Responder<'r, 'static> for DirectStream {
    fn respond_to(self, _request: &'r Request<'_>) -> response::Result<'static> {
        // Get the current track from the playlist
        let result = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
        
        if let Some(track) = result {
            info!("Streaming track: {} by {}", track.title, track.artist);
            
            // Create the full path to the MP3 file
            let file_path = config::MUSIC_FOLDER.join(&track.path);
            
            if !Path::new(&file_path).exists() {
                error!("Track file not found: {}", file_path.display());
                return Err(Status::NotFound);
            }
            
            // Calculate the position or use the one from the stream manager
            let server_position = self.requested_position
                .unwrap_or_else(|| self.stream_manager.get_playback_position());
            
            // Get file metadata for size and calculate bitrate
            let file_size = match std::fs::metadata(&file_path) {
                Ok(metadata) => metadata.len(),
                Err(e) => {
                    error!("Error getting file metadata: {}", e);
                    return Err(Status::InternalServerError);
                }
            };
            
            // Calculate bitrate from file size and track duration
            let bitrate_kbps = if track.duration > 0 && file_size > 0 {
                (file_size * 8) / (track.duration * 1000)
            } else {
                128 // Default 128kbps if we can't calculate
            };
            
            // Calculate byte position
            let byte_position = Self::time_to_bytes(server_position, bitrate_kbps);
            
            // Ensure we don't seek past the end of the file
            let start_position = if byte_position >= file_size {
                info!("Calculated position ({} bytes) exceeds file size, starting from beginning", byte_position);
                0
            } else {
                byte_position
            };
                
            info!("Starting stream at position {}s (byte offset: {})", server_position, start_position);
            
            // Instead of streaming manually, just return the file directly
            // Rocket will handle the streaming for us
            let mut response = Response::build()
                .header(ContentType::new("audio", "mpeg"))
                .header(Header::new("Accept-Ranges", "bytes"))
                .header(Header::new("Content-Disposition", "inline"))
                .header(Header::new("Cache-Control", "no-cache, no-store, must-revalidate"))
                .header(Header::new("Pragma", "no-cache"))
                .header(Header::new("Access-Control-Allow-Origin", "*"))
                .header(Header::new("X-Content-Type-Options", "nosniff"))
                .header(Header::new("X-Server-Position", server_position.to_string()))
                .header(Header::new("X-Byte-Position", start_position.to_string()));
                
            // Create a custom body from the file directly
            if let Ok(mut file) = File::open(&file_path) {
                // Seek to start position if needed
                if start_position > 0 {
                    if let Err(e) = file.seek(SeekFrom::Start(start_position)) {
                        error!("Error seeking to position in file: {}", e);
                    }
                }
                
                // Read the entire file into memory
                // For large files this might not be the best approach, but it works for MP3s
                let mut body = Vec::new();
                if let Err(e) = file.read_to_end(&mut body) {
                    error!("Error reading file: {}", e);
                    return Err(Status::InternalServerError);
                }
                
                // Create the response with the body
                return Ok(response.sized_body(body.len(), io::Cursor::new(body)).finalize());
            } else {
                error!("Error opening file: {}", file_path.display());
                return Err(Status::InternalServerError);
            }
        } else {
            error!("No track available to stream");
            Err(Status::NotFound)
        }
    }
}

// Handle the direct streaming endpoint
#[rocket::get("/direct-stream?<position>")]
pub fn direct_stream(
    position: Option<u64>,
    stream_manager: &State<Arc<StreamManager>>
) -> DirectStream {
    // Update listener count
    stream_manager.increment_listener_count();
    
    // Return the direct stream handler with position
    DirectStream::new(
        stream_manager.inner().clone(), 
        position
    )
}

// Status endpoint remains the same
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
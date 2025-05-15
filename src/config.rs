use std::path::PathBuf;
use std::env;
use lazy_static::lazy_static;

lazy_static! {
    // Base directory
    pub static ref BASE_DIR: PathBuf = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    
    // Music folder
    pub static ref MUSIC_FOLDER: PathBuf = BASE_DIR.join("music");
    
    // Playlist file
    pub static ref PLAYLIST_FILE: PathBuf = BASE_DIR.join("playlist.json");
}

// Stream configuration - unified constants
pub const CHUNK_SIZE: usize = 1024 * 16;  // Chunk size for streaming (16 KB)
pub const BUFFER_SIZE: usize = 1024 * 256;  // Increased buffer size (256 KB)
pub const STREAM_CACHE_TIME: u64 = 60;  // Seconds to cache stream chunks

// Buffer management - now centralized in config
pub const MAX_RECENT_CHUNKS: usize = 100;  // Increased: Number of chunks to save for new clients
pub const INITIAL_CHUNKS_TO_SEND: usize = 30;  // Increased: Number of chunks to send on connection
pub const BROADCAST_BUFFER_SIZE: usize = 64;  // Increased: Size of buffer for broadcasting
pub const MIN_BUFFER_CHUNKS: usize = 20;  // Increased: Minimum chunks to buffer before playback
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 10;  // Reduced: Delay during buffer underruns

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 50;  // Maximum number of simultaneous connections

// Adaptive buffering configuration
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 10;  // Extra chunks for low bitrate files
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 30;  // Extra chunks for high bitrate files

// WebSocket connection management
pub const WS_PING_INTERVAL_MS: u64 = 5000;  // Send ping every 5 seconds
pub const WS_TIMEOUT_SECS: u64 = 60;  // Client timeout after 60 seconds of inactivity
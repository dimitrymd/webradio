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

// Stream configuration
pub const CHUNK_SIZE: usize = 1024 * 16;  // Chunk size for streaming (16 KB)
pub const BUFFER_SIZE: usize = 1024 * 64;  // Buffer size for multi-user streaming (64 KB)
pub const STREAM_CACHE_TIME: u64 = 60;  // Seconds to cache stream chunks

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 50;  // Maximum number of simultaneous connections

// Adaptive buffering configuration
pub const ADAPTIVE_BUFFERING: bool = true;
pub const MIN_BUFFER_CHUNKS: usize = 10;
pub const MAX_BUFFER_DURATION_SECS: u64 = 30;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
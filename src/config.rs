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

// Stream configuration - increased buffer sizes and chunk sizes
pub const CHUNK_SIZE: usize = 1024 * 32;  // Increased chunk size (32 KB)
pub const BUFFER_SIZE: usize = 1024 * 512;  // Significantly increased buffer size (512 KB)
pub const STREAM_CACHE_TIME: u64 = 120;  // Doubled cache time (120 seconds)

// Buffer management - increased buffer settings
pub const MAX_RECENT_CHUNKS: usize = 200;  // Doubled from 100
pub const INITIAL_CHUNKS_TO_SEND: usize = 60;  // Doubled from 30
pub const BROADCAST_BUFFER_SIZE: usize = 128;  // Doubled from 64
pub const MIN_BUFFER_CHUNKS: usize = 40;  // Doubled from 20
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 5;  // Halved from 10ms for faster recovery

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 100;  // Doubled from 50

// Adaptive buffering configuration
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 20;  // Doubled from 10
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 60;  // Doubled from 30

// WebSocket connection management
pub const WS_PING_INTERVAL_MS: u64 = 3000;  // More frequent pings (from 5000 to 3000)
pub const WS_TIMEOUT_SECS: u64 = 90;  // Longer timeout (from 60 to 90 seconds)

// Direct streaming specific configurations
pub const DIRECT_STREAM_BUFFER_SIZE: usize = 1024 * 64;  // 64KB for direct streaming buffer
pub const IOS_INITIAL_BUFFER_SIZE: usize = 60;  // Number of chunks to buffer for iOS initially
pub const SAFARI_INITIAL_BUFFER_SIZE: usize = 45;  // Number of chunks for Safari
pub const MOBILE_INITIAL_BUFFER_SIZE: usize = 30;  // Number of chunks for mobile devices
pub const DESKTOP_INITIAL_BUFFER_SIZE: usize = 20;  // Number of chunks for desktop browsers

// Broadcast channel capacity - increased for better performance
pub const BROADCAST_CHANNEL_CAPACITY: usize = 4000;  // Up from default of 2000
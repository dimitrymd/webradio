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

// Enhanced stream configuration for better position synchronization
pub const CHUNK_SIZE: usize = 1024 * 16;  // Smaller chunks for better position accuracy (16 KB)
pub const BUFFER_SIZE: usize = 1024 * 256;  // Balanced buffer size (256 KB)
pub const STREAM_CACHE_TIME: u64 = 60;  // Reduced cache time for more responsive sync (60 seconds)

// Enhanced buffer management for position accuracy
pub const MAX_RECENT_CHUNKS: usize = 150;  // Balanced chunk history
pub const INITIAL_CHUNKS_TO_SEND: usize = 40;  // Good initial buffer
pub const BROADCAST_BUFFER_SIZE: usize = 96;  // Optimized broadcast buffer
pub const MIN_BUFFER_CHUNKS: usize = 20;  // Minimum safety buffer
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 2;  // Very fast recovery for position accuracy

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 100;

// Enhanced adaptive buffering for position synchronization
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 15;  // Conservative for low bitrate
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 45;  // More buffer for high bitrate

// WebSocket connection management (if needed for future features)
pub const WS_PING_INTERVAL_MS: u64 = 2000;  // More frequent for position sync
pub const WS_TIMEOUT_SECS: u64 = 90;

// Enhanced direct streaming configurations for position accuracy
pub const DIRECT_STREAM_BUFFER_SIZE: usize = 1024 * 32;  // 32KB for optimal streaming
pub const IOS_INITIAL_BUFFER_SIZE: usize = 45;  // Optimized for iOS position sync
pub const SAFARI_INITIAL_BUFFER_SIZE: usize = 35;  // Safari optimization
pub const MOBILE_INITIAL_BUFFER_SIZE: usize = 25;  // Mobile device optimization
pub const DESKTOP_INITIAL_BUFFER_SIZE: usize = 20;  // Desktop browser optimization

// Enhanced broadcast channel capacity
pub const BROADCAST_CHANNEL_CAPACITY: usize = 3000;  // Balanced capacity

// New position synchronization constants
pub const POSITION_UPDATE_INTERVAL_MS: u64 = 100;  // Update position every 100ms
pub const POSITION_SYNC_TOLERANCE_SECONDS: u64 = 2;  // 2 second tolerance for drift
pub const MP3_FRAME_ALIGNMENT: bool = true;  // Enable MP3 frame boundary alignment
pub const ID3_DETECTION: bool = true;  // Enable accurate ID3 tag size detection

// MP3 streaming optimization constants
pub const DEFAULT_MP3_FRAME_SIZE: u64 = 144;  // Standard MP3 frame size at 44.1kHz
pub const MIN_MP3_FRAME_SIZE: u64 = 96;   // Minimum frame size
pub const MAX_MP3_FRAME_SIZE: u64 = 1728; // Maximum frame size
pub const DEFAULT_ID3_OFFSET: u64 = 1024; // Conservative ID3 offset
pub const MAX_ID3_TAG_SIZE: u64 = 128 * 1024; // 128KB max ID3 tag size

// Position persistence and client sync
pub const POSITION_SAVE_INTERVAL_MS: u64 = 5000;  // Save position every 5 seconds
pub const MAX_RECONNECT_GAP_MS: u64 = 10000;  // 10 second max for position continuity
pub const POSITION_DRIFT_CORRECTION_FACTOR: f64 = 0.1;  // 10% correction per update
pub const CLIENT_POSITION_SYNC_ENABLED: bool = true;

// Enhanced error handling and recovery
pub const MAX_POSITION_CORRECTION_ATTEMPTS: u8 = 3;
pub const POSITION_VALIDATION_STRICT: bool = true;
pub const TRACK_TRANSITION_BUFFER_MS: u64 = 500;  // Buffer time between tracks

// Debug and monitoring
pub const POSITION_DEBUG_LOGGING: bool = true;
pub const DRIFT_DETECTION_LOGGING: bool = true;
pub const PERFORMANCE_MONITORING: bool = true;
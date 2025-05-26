// src/config.rs - Optimized configuration for all platforms

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

// Optimized chunk sizes for different platforms
pub const CHUNK_SIZE: usize = 1024 * 8;           // 8KB base chunk size
pub const BUFFER_SIZE: usize = 1024 * 128;        // 128KB buffer

// Platform-specific chunk sizes
pub const IOS_CHUNK_SIZE: usize = 1024 * 64;      // 64KB for iOS (reduced from 256KB)
pub const IOS_MAX_BUFFER: usize = 1024 * 64;      // 64KB max buffer for iOS
pub const ANDROID_CHUNK_SIZE: usize = 1024 * 32;  // 32KB for Android (reduced from 128KB)
pub const DESKTOP_CHUNK_SIZE: usize = 1024 * 128; // 128KB for desktop (reduced from 192KB)

// Stream configuration
pub const STREAM_CACHE_TIME: u64 = 30;            // Reduced cache time for better responsiveness

// Buffer management
pub const MAX_RECENT_CHUNKS: usize = 100;         // Reduced for better memory usage
pub const INITIAL_CHUNKS_TO_SEND: usize = 20;     // Reduced initial buffer
pub const BROADCAST_BUFFER_SIZE: usize = 64;      // Reduced broadcast buffer
pub const MIN_BUFFER_CHUNKS: usize = 10;          // Minimum safety buffer
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 5;    // Fast recovery

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 1000;

// Connection management
pub const CONNECTION_TIMEOUT_SECS: u64 = 90;
pub const HEARTBEAT_INTERVAL_SECS: u64 = 15;
pub const STALE_CONNECTION_CHECK_SECS: u64 = 120;

// WebSocket configuration (if needed)
pub const WS_PING_INTERVAL_MS: u64 = 5000;
pub const WS_TIMEOUT_SECS: u64 = 90;

// Broadcast channel capacity
pub const BROADCAST_CHANNEL_CAPACITY: usize = 2000;

// Position synchronization
pub const POSITION_UPDATE_INTERVAL_MS: u64 = 100;
pub const POSITION_SYNC_TOLERANCE_SECONDS: u64 = 2;
pub const POSITION_SAVE_INTERVAL_MS: u64 = 5000;
pub const MAX_RECONNECT_GAP_MS: u64 = 10000;

// MP3 streaming optimization
pub const MP3_FRAME_ALIGNMENT: bool = true;
pub const FRAME_BOUNDARY_ALIGNMENT: bool = true;
pub const ID3_DETECTION: bool = true;
pub const VBR_DETECTION_ENABLED: bool = true;

// MP3 frame constants
pub const DEFAULT_MP3_FRAME_SIZE: u64 = 144;
pub const MIN_MP3_FRAME_SIZE: u64 = 96;
pub const MAX_MP3_FRAME_SIZE: u64 = 1728;
pub const DEFAULT_ID3_OFFSET: u64 = 1024;
pub const MAX_ID3_TAG_SIZE: u64 = 128 * 1024;

// MP3 sync pattern
pub const MP3_FRAME_SYNC_BYTES: [u8; 2] = [0xFF, 0xE0];

// Track transition
pub const TRACK_TRANSITION_BUFFER_MS: u64 = 100;
pub const TRACK_PRELOAD_SECONDS: u64 = 5;

// Error handling
pub const MAX_POSITION_CORRECTION_ATTEMPTS: u8 = 3;
pub const POSITION_VALIDATION_STRICT: bool = true;

// Debug and monitoring
pub const POSITION_DEBUG_LOGGING: bool = false;
pub const DRIFT_DETECTION_LOGGING: bool = false;
pub const PERFORMANCE_MONITORING: bool = true;

// Adaptive buffering thresholds
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000;
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 10;
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 30;

// Client position sync (disabled in radio mode)
pub const CLIENT_POSITION_SYNC_ENABLED: bool = false;
pub const POSITION_DRIFT_CORRECTION_FACTOR: f64 = 0.0;

// Platform-specific buffer sizes
pub const IOS_INITIAL_BUFFER_SIZE: usize = 20;
pub const SAFARI_INITIAL_BUFFER_SIZE: usize = 25;
pub const MOBILE_INITIAL_BUFFER_SIZE: usize = 15;
pub const DESKTOP_INITIAL_BUFFER_SIZE: usize = 30;

// Direct streaming buffer sizes
pub const DIRECT_STREAM_BUFFER_SIZE: usize = 1024 * 64;

// Network quality detection
pub const ENABLE_NETWORK_QUALITY_DETECTION: bool = true;
pub const POOR_NETWORK_EXTRA_BUFFER_MS: u64 = 2000;

// Radio mode specific
pub const RADIO_MODE: bool = true;
pub const RADIO_SYNC_INTERVAL_MS: u64 = 5000;
pub const RADIO_POSITION_AUTHORITY: &str = "server";
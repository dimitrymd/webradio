// src/config.rs - CPU Optimized configuration

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

// CPU-optimized chunk sizes - better balance for smooth playback
pub const CHUNK_SIZE: usize = 1024 * 8;           // Back to 8KB for smoother timing
pub const BUFFER_SIZE: usize = 1024 * 64;         // 64KB buffer

// Platform-specific chunk sizes - optimized for smooth playback
pub const IOS_CHUNK_SIZE: usize = 1024 * 16;      // 16KB for iOS
pub const IOS_MAX_BUFFER: usize = 1024 * 32;      // 32KB max buffer for iOS
pub const ANDROID_CHUNK_SIZE: usize = 1024 * 8;   // 8KB for Android (smoother)
pub const DESKTOP_CHUNK_SIZE: usize = 1024 * 16;  // 16KB for desktop

// Stream configuration - balanced for performance and smoothness
pub const STREAM_CACHE_TIME: u64 = 30;            // Balanced cache time

// Buffer management - smaller buffers = less memory = better CPU cache performance
pub const MAX_RECENT_CHUNKS: usize = 50;          // Reduced from 100
pub const INITIAL_CHUNKS_TO_SEND: usize = 10;     // Reduced from 20
pub const BROADCAST_BUFFER_SIZE: usize = 32;      // Reduced from 64
pub const MIN_BUFFER_CHUNKS: usize = 5;           // Reduced minimum buffer
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 10;   // Slightly increased for stability

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 1000;

// Connection management - longer intervals to reduce CPU load
pub const CONNECTION_TIMEOUT_SECS: u64 = 120;     // Increased from 90
pub const HEARTBEAT_INTERVAL_SECS: u64 = 30;      // Increased from 15
pub const STALE_CONNECTION_CHECK_SECS: u64 = 180; // Increased from 120

// WebSocket configuration (if needed) - longer intervals
pub const WS_PING_INTERVAL_MS: u64 = 10000;       // Increased from 5000
pub const WS_TIMEOUT_SECS: u64 = 120;             // Increased from 90

// Broadcast channel capacity - smaller for better performance
pub const BROADCAST_CHANNEL_CAPACITY: usize = 1000; // Reduced from 2000

// Position synchronization - less frequent updates
pub const POSITION_UPDATE_INTERVAL_MS: u64 = 200;   // Increased from 100
pub const POSITION_SYNC_TOLERANCE_SECONDS: u64 = 3; // Increased tolerance
pub const POSITION_SAVE_INTERVAL_MS: u64 = 10000;   // Increased from 5000
pub const MAX_RECONNECT_GAP_MS: u64 = 15000;        // Increased from 10000

// MP3 streaming optimization
pub const MP3_FRAME_ALIGNMENT: bool = true;
pub const FRAME_BOUNDARY_ALIGNMENT: bool = true;
pub const ID3_DETECTION: bool = true;
pub const VBR_DETECTION_ENABLED: bool = false;      // Disabled for CPU savings

// MP3 frame constants
pub const DEFAULT_MP3_FRAME_SIZE: u64 = 144;
pub const MIN_MP3_FRAME_SIZE: u64 = 96;
pub const MAX_MP3_FRAME_SIZE: u64 = 1728;
pub const DEFAULT_ID3_OFFSET: u64 = 1024;
pub const MAX_ID3_TAG_SIZE: u64 = 128 * 1024;

// MP3 sync pattern
pub const MP3_FRAME_SYNC_BYTES: [u8; 2] = [0xFF, 0xE0];

// Track transition - longer buffers for stability
pub const TRACK_TRANSITION_BUFFER_MS: u64 = 200;    // Increased from 100
pub const TRACK_PRELOAD_SECONDS: u64 = 3;           // Reduced from 5

// Error handling
pub const MAX_POSITION_CORRECTION_ATTEMPTS: u8 = 2; // Reduced from 3
pub const POSITION_VALIDATION_STRICT: bool = false; // Relaxed for performance

// Debug and monitoring - reduced logging for CPU savings
pub const POSITION_DEBUG_LOGGING: bool = false;
pub const DRIFT_DETECTION_LOGGING: bool = false;
pub const PERFORMANCE_MONITORING: bool = false;     // Disabled for CPU savings

// Adaptive buffering thresholds - simplified
pub const ADAPTIVE_BUFFERING: bool = false;         // Disabled for CPU savings
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000;
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 5;      // Reduced from 10
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 15;    // Reduced from 30

// Client position sync (disabled in radio mode)
pub const CLIENT_POSITION_SYNC_ENABLED: bool = false;
pub const POSITION_DRIFT_CORRECTION_FACTOR: f64 = 0.0;

// Platform-specific buffer sizes - optimized for CPU
pub const IOS_INITIAL_BUFFER_SIZE: usize = 10;      // Reduced from 20
pub const SAFARI_INITIAL_BUFFER_SIZE: usize = 12;   // Reduced from 25
pub const MOBILE_INITIAL_BUFFER_SIZE: usize = 8;    // Reduced from 15
pub const DESKTOP_INITIAL_BUFFER_SIZE: usize = 15;  // Reduced from 30

// Direct streaming buffer sizes
pub const DIRECT_STREAM_BUFFER_SIZE: usize = 1024 * 32; // Reduced from 64KB

// Network quality detection - disabled for CPU savings
pub const ENABLE_NETWORK_QUALITY_DETECTION: bool = false;
pub const POOR_NETWORK_EXTRA_BUFFER_MS: u64 = 1000; // Reduced from 2000

// Radio mode specific
pub const RADIO_MODE: bool = true;
pub const RADIO_SYNC_INTERVAL_MS: u64 = 10000;      // Increased from 5000
pub const RADIO_POSITION_AUTHORITY: &str = "server";

// CPU optimization flags
pub const ENABLE_DETAILED_LOGGING: bool = false;    // Disable verbose logging
pub const ENABLE_METRICS_COLLECTION: bool = false;  // Disable metrics for CPU savings
pub const USE_RELAXED_MEMORY_ORDERING: bool = true; // Use relaxed atomic ordering
pub const BATCH_OPERATIONS: bool = true;            // Enable batch operations
pub const REDUCE_SYSCALLS: bool = true;             // Minimize system calls

// File I/O optimization
pub const PLAYLIST_CACHE_DURATION_SECS: u64 = 30;   // Cache playlist reads
pub const FILE_BUFFER_SIZE: usize = 1024 * 64;      // 64KB file buffer
pub const REDUCE_FILE_STAT_CALLS: bool = true;      // Cache file existence checks

// Threading optimization
pub const THREAD_SLEEP_PRECISION_MS: u64 = 10;      // Reduced sleep precision
pub const BACKGROUND_TASK_INTERVAL_MS: u64 = 1000;  // Less frequent background tasks
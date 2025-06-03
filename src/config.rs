// src/config.rs - Ultra CPU-optimized configuration

use std::path::PathBuf;
use std::env;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref BASE_DIR: PathBuf = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    pub static ref MUSIC_FOLDER: PathBuf = BASE_DIR.join("music");
    pub static ref PLAYLIST_FILE: PathBuf = BASE_DIR.join("playlist.json");
}

// ===== BALANCED CPU OPTIMIZATION SETTINGS =====

// Balanced chunks for smooth playback with lower CPU
pub const CHUNK_SIZE: usize = 1024 * 64;          // 64KB chunks
pub const BUFFER_SIZE: usize = 1024 * 128;        // 128KB buffer

// Platform-specific chunk sizes
pub const IOS_CHUNK_SIZE: usize = 1024 * 32;      // 32KB for iOS
pub const IOS_MAX_BUFFER: usize = 1024 * 64;      // 64KB max
pub const ANDROID_CHUNK_SIZE: usize = 1024 * 32;  // 32KB for Android
pub const DESKTOP_CHUNK_SIZE: usize = 1024 * 64;  // 64KB for desktop

// Stream configuration
pub const STREAM_CACHE_TIME: u64 = 120;           // 2 minutes cache

// Buffer management - balanced for smooth playback
pub const MAX_RECENT_CHUNKS: usize = 25;          // Reasonable buffer
pub const INITIAL_CHUNKS_TO_SEND: usize = 3;      // Quick start
pub const BROADCAST_BUFFER_SIZE: usize = 50;      // Good buffer size
pub const MIN_BUFFER_CHUNKS: usize = 3;           // Minimum buffer
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 50;  // Quick recovery

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 1000;

// Connection management - very long intervals
pub const CONNECTION_TIMEOUT_SECS: u64 = 600;     // 10 minutes
pub const HEARTBEAT_INTERVAL_SECS: u64 = 120;     // 2 minutes
pub const STALE_CONNECTION_CHECK_SECS: u64 = 600; // 10 minutes

// WebSocket configuration
pub const WS_PING_INTERVAL_MS: u64 = 60000;       // 1 minute
pub const WS_TIMEOUT_SECS: u64 = 600;             // 10 minutes

// Broadcast channel capacity
pub const BROADCAST_CHANNEL_CAPACITY: usize = 100; // Minimal

// Position synchronization - very rare
pub const POSITION_UPDATE_INTERVAL_MS: u64 = 2000;  // 2 seconds
pub const POSITION_SYNC_TOLERANCE_SECONDS: u64 = 10;
pub const POSITION_SAVE_INTERVAL_MS: u64 = 60000;   // 1 minute
pub const MAX_RECONNECT_GAP_MS: u64 = 60000;        // 1 minute

// All features disabled
pub const MP3_FRAME_ALIGNMENT: bool = false;
pub const FRAME_BOUNDARY_ALIGNMENT: bool = false;
pub const ID3_DETECTION: bool = false;              // Even this
pub const VBR_DETECTION_ENABLED: bool = false;

// Track transition
pub const TRACK_TRANSITION_BUFFER_MS: u64 = 1000;  // 1 second
pub const TRACK_PRELOAD_SECONDS: u64 = 0;          // No preloading

// Error handling
pub const MAX_POSITION_CORRECTION_ATTEMPTS: u8 = 0; // None
pub const POSITION_VALIDATION_STRICT: bool = false;

// All monitoring disabled
pub const POSITION_DEBUG_LOGGING: bool = false;
pub const DRIFT_DETECTION_LOGGING: bool = false;
pub const PERFORMANCE_MONITORING: bool = false;
pub const ENABLE_DETAILED_LOGGING: bool = false;
pub const ENABLE_METRICS_COLLECTION: bool = false;

// Adaptive buffering - disabled
pub const ADAPTIVE_BUFFERING: bool = false;
pub const HIGH_BITRATE_THRESHOLD: u64 = 256000;
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 1;
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 2;

// Client position sync - disabled
pub const CLIENT_POSITION_SYNC_ENABLED: bool = false;
pub const POSITION_DRIFT_CORRECTION_FACTOR: f64 = 0.0;

// Platform-specific buffer sizes - minimal
pub const IOS_INITIAL_BUFFER_SIZE: usize = 1;
pub const SAFARI_INITIAL_BUFFER_SIZE: usize = 1;
pub const MOBILE_INITIAL_BUFFER_SIZE: usize = 1;
pub const DESKTOP_INITIAL_BUFFER_SIZE: usize = 2;

// Direct streaming buffer
pub const DIRECT_STREAM_BUFFER_SIZE: usize = 1024 * 64; // 64KB

// Network quality detection - disabled
pub const ENABLE_NETWORK_QUALITY_DETECTION: bool = false;
pub const POOR_NETWORK_EXTRA_BUFFER_MS: u64 = 0;

// Radio mode
pub const RADIO_MODE: bool = true;
pub const RADIO_SYNC_INTERVAL_MS: u64 = 60000;     // 1 minute
pub const RADIO_POSITION_AUTHORITY: &str = "server";

// CPU optimization flags
pub const USE_RELAXED_MEMORY_ORDERING: bool = true;
pub const BATCH_OPERATIONS: bool = true;
pub const REDUCE_SYSCALLS: bool = true;

// File I/O optimization
pub const PLAYLIST_CACHE_DURATION_SECS: u64 = 120;  // 2 minutes
pub const FILE_BUFFER_SIZE: usize = 1024 * 256;     // 256KB
pub const REDUCE_FILE_STAT_CALLS: bool = true;

// Threading optimization
pub const THREAD_SLEEP_PRECISION_MS: u64 = 100;     // Very low precision
pub const BACKGROUND_TASK_INTERVAL_MS: u64 = 10000; // 10 seconds

// Logging
pub const LOG_LEVEL: &str = "error";                // Only errors
pub const SUPPRESS_PROGRESS_LOGS: bool = true;
pub const SUPPRESS_CONNECTION_LOGS: bool = true;

// Memory optimization
pub const USE_JEMALLOC: bool = false;
pub const PREALLOCATE_BUFFERS: bool = true;

// Thread pool settings
pub const WORKER_THREADS: usize = 1;                // Single thread
pub const MAX_BLOCKING_THREADS: usize = 2;          // Minimal

// HTTP optimization
pub const KEEP_ALIVE_TIMEOUT: u32 = 600;           // 10 minutes
pub const REQUEST_TIMEOUT: u64 = 60;               // 1 minute
pub const RESPONSE_COMPRESSION: bool = false;      // No compression
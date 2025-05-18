// src/config.rs - Improved configuration with better defaults

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

// Stream configuration - improved for better buffering
pub const CHUNK_SIZE: usize = 1024 * 64;  // Increased from 32KB to 64KB
pub const BUFFER_SIZE: usize = 1024 * 1024 * 2;  // Increased to 2MB for better buffering
pub const STREAM_CACHE_TIME: u64 = 120;  // Increased to 120 seconds to cache stream chunks

// Buffer management - increased sizes for smoother playback
pub const MAX_RECENT_CHUNKS: usize = 500;  // Increased number of chunks to save for new clients
pub const INITIAL_CHUNKS_TO_SEND: usize = 150;  // More chunks for smoother start
pub const BROADCAST_BUFFER_SIZE: usize = 300;  // Larger buffer for broadcasting
pub const MIN_BUFFER_CHUNKS: usize = 100;  // More chunks before playback starts
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 5;  // Reduced delay during buffer underruns

// Add new buffering constants for performance tuning
pub const STREAM_BUFFER_MULTIPLIER: usize = 3;  // How many times buffer should exceed real-time needs
pub const STREAM_CHUNK_SEND_RATE_MULTIPLIER: f64 = 1.2;  // Send chunks 20% faster than real-time

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 100;  // Increased for more connections

// Adaptive buffering configuration
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 30;  // Increased from 20
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 80;  // Increased from 60

// Network condition adaptation
pub const NETWORK_CONDITION_CHECK_INTERVAL_MS: u64 = 5000;  // Check every 5 seconds
pub const LOW_LATENCY_MODE: bool = false;  // Disable low latency mode for more reliable playback
pub const ADAPTIVE_BUFFER_SIZE: bool = true;  // Dynamically adjust buffer based on conditions

// WebSocket connection management
pub const WS_BINARY_FRAGMENT_SIZE: usize = 16 * 1024;  // Optimize WebSocket frame size
pub const WS_PING_INTERVAL_MS: u64 = 5000;  // Pings every 5 seconds
pub const WS_TIMEOUT_SECS: u64 = 15;  // Longer timeout for better reliability

// Track transition handling
pub const TRACK_TRANSITION_BUFFER_CHUNKS: usize = 50;  // Pre-buffer chunks for next track
pub const PRE_BUFFER_PERCENTAGE: u8 = 80;  // Start pre-buffering at 80% of current track
pub const TRACK_END_BUFFER_TIME_SEC: u64 = 3;  // Keep 3 seconds of buffer at track end
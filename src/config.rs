// src/config.rs - Complete updated file

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
pub const BUFFER_SIZE: usize = 1024 * 1024;  // Increased to 1MB for better buffering
pub const STREAM_CACHE_TIME: u64 = 60;  // Seconds to cache stream chunks

// Buffer management - increased sizes for smoother playback
pub const MAX_RECENT_CHUNKS: usize = 400;  // Increased number of chunks to save for new clients
pub const INITIAL_CHUNKS_TO_SEND: usize = 120;  // More chunks for smoother start
pub const BROADCAST_BUFFER_SIZE: usize = 200;  // Larger buffer for broadcasting
pub const MIN_BUFFER_CHUNKS: usize = 80;  // More chunks before playback starts
pub const UNDERRUN_RECOVERY_DELAY_MS: u64 = 5;  // Reduced delay during buffer underruns

// Add new buffering constants for performance tuning
pub const STREAM_BUFFER_MULTIPLIER: usize = 3;  // How many times buffer should exceed real-time needs
pub const STREAM_CHUNK_SEND_RATE_MULTIPLIER: f64 = 1.3;  // Send chunks 30% faster than real-time

// Server configuration
pub const PORT: u16 = 8000;
pub const HOST: &str = "0.0.0.0";
pub const MAX_CONCURRENT_USERS: usize = 50;  // Maximum number of simultaneous connections

// Adaptive buffering configuration
pub const ADAPTIVE_BUFFERING: bool = true;
pub const HIGH_BITRATE_THRESHOLD: u64 = 192000; // 192kbps
pub const LOW_BITRATE_EXTRA_CHUNKS: usize = 20;  // Increased from 10
pub const HIGH_BITRATE_EXTRA_CHUNKS: usize = 60;  // Increased from 30

// Network condition adaptation
pub const NETWORK_CONDITION_CHECK_INTERVAL_MS: u64 = 5000;  // Check every 5 seconds
pub const LOW_LATENCY_MODE: bool = true;  // Enable low latency optimizations
pub const ADAPTIVE_BUFFER_SIZE: bool = true;  // Dynamically adjust buffer based on conditions

// WebSocket connection management
pub const WS_BINARY_FRAGMENT_SIZE: usize = 16 * 1024;  // Optimize WebSocket frame size
pub const WS_PING_INTERVAL_MS: u64 = 2000;  // More frequent pings
pub const WS_TIMEOUT_SECS: u64 = 10;  // Shorter timeout for faster reconnection

// Transcoding configuration
pub const ENABLE_TRANSCODING: bool = true;  // Enable/disable transcoding to Opus
pub const OPUS_CHUNK_SIZE: usize = 1024 * 4;  // Smaller chunk size for Opus (4KB)
pub const OPUS_BUFFER_SIZE: usize = 1024 * 256;  // Increased buffer size for Opus (256KB)
pub const OPUS_INITIAL_CHUNKS_TO_SEND: usize = 80;  // More initial chunks for Opus streaming

// iOS-specific optimizations
pub const IOS_BUFFER_SIZE_FACTOR: f32 = 1.5;  // Increase buffer for iOS
pub const IOS_OPUS_PACKET_INTERVAL_MS: u64 = 10;  // More frequent Opus packets
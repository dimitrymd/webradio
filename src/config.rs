use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub music_dir: PathBuf,

    // Streaming configuration
    pub initial_buffer_kb: usize,      // Initial buffer size for new listeners (KB)
    pub minimum_buffer_kb: usize,      // Minimum buffer before starting playback (KB)
    pub chunk_interval_ms: u64,        // Interval between chunks (milliseconds)
    pub stream_rate_multiplier: f64,   // Stream faster than bitrate to build client buffers (1.10 = 10% faster)
    pub initial_buffer_timeout_ms: u64, // Timeout for initial buffer collection
    pub broadcast_channel_capacity: usize, // Capacity of broadcast channel
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000),
            music_dir: std::env::var("MUSIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("music")),

            // Streaming defaults optimized for stable radio streaming
            initial_buffer_kb: std::env::var("INITIAL_BUFFER_KB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),  // 120KB = ~5 seconds at 192kbps

            minimum_buffer_kb: std::env::var("MINIMUM_BUFFER_KB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(80),   // 80KB = ~3.3 seconds minimum (ensure solid buffer)

            chunk_interval_ms: std::env::var("CHUNK_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),  // 100ms chunks (iOS compatible)

            stream_rate_multiplier: std::env::var("STREAM_RATE_MULTIPLIER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.10), // 10% faster than bitrate

            initial_buffer_timeout_ms: std::env::var("INITIAL_BUFFER_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6000), // 6 seconds to collect initial buffer (120KB at 211kbps)

            broadcast_channel_capacity: std::env::var("BROADCAST_CHANNEL_CAPACITY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(32768), // 32K messages capacity
        }
    }
}
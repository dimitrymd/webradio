use std::path::PathBuf;

/// Configuration for the WebRadio server
/// Can be loaded from environment variables using `Config::from_env()`
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_config_defaults() {
        // Clear any environment variables that might affect the test
        env::remove_var("HOST");
        env::remove_var("PORT");
        env::remove_var("MUSIC_DIR");
        env::remove_var("INITIAL_BUFFER_KB");
        env::remove_var("MINIMUM_BUFFER_KB");
        env::remove_var("CHUNK_INTERVAL_MS");
        env::remove_var("STREAM_RATE_MULTIPLIER");
        env::remove_var("INITIAL_BUFFER_TIMEOUT_MS");
        env::remove_var("BROADCAST_CHANNEL_CAPACITY");

        let config = Config::from_env();

        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8000);
        assert_eq!(config.music_dir, PathBuf::from("music"));
        assert_eq!(config.initial_buffer_kb, 120);
        assert_eq!(config.minimum_buffer_kb, 80);
        assert_eq!(config.chunk_interval_ms, 100);
        assert_eq!(config.stream_rate_multiplier, 1.10);
        assert_eq!(config.initial_buffer_timeout_ms, 6000);
        assert_eq!(config.broadcast_channel_capacity, 32768);
    }

    #[test]
    fn test_config_from_env() {
        env::set_var("HOST", "127.0.0.1");
        env::set_var("PORT", "9000");
        env::set_var("MUSIC_DIR", "/custom/music");
        env::set_var("INITIAL_BUFFER_KB", "200");
        env::set_var("MINIMUM_BUFFER_KB", "100");
        env::set_var("CHUNK_INTERVAL_MS", "50");
        env::set_var("STREAM_RATE_MULTIPLIER", "1.15");
        env::set_var("INITIAL_BUFFER_TIMEOUT_MS", "5000");
        env::set_var("BROADCAST_CHANNEL_CAPACITY", "16384");

        let config = Config::from_env();

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 9000);
        assert_eq!(config.music_dir, PathBuf::from("/custom/music"));
        assert_eq!(config.initial_buffer_kb, 200);
        assert_eq!(config.minimum_buffer_kb, 100);
        assert_eq!(config.chunk_interval_ms, 50);
        assert_eq!(config.stream_rate_multiplier, 1.15);
        assert_eq!(config.initial_buffer_timeout_ms, 5000);
        assert_eq!(config.broadcast_channel_capacity, 16384);

        // Cleanup
        env::remove_var("HOST");
        env::remove_var("PORT");
        env::remove_var("MUSIC_DIR");
        env::remove_var("INITIAL_BUFFER_KB");
        env::remove_var("MINIMUM_BUFFER_KB");
        env::remove_var("CHUNK_INTERVAL_MS");
        env::remove_var("STREAM_RATE_MULTIPLIER");
        env::remove_var("INITIAL_BUFFER_TIMEOUT_MS");
        env::remove_var("BROADCAST_CHANNEL_CAPACITY");
    }

    #[test]
    fn test_config_invalid_port_uses_default() {
        env::set_var("PORT", "invalid");

        let config = Config::from_env();
        assert_eq!(config.port, 8000);

        env::remove_var("PORT");
    }

    #[test]
    fn test_config_buffer_calculations() {
        env::remove_var("INITIAL_BUFFER_KB");
        env::remove_var("MINIMUM_BUFFER_KB");

        let config = Config::from_env();

        // At 192kbps, 24KB = 1 second
        let initial_buffer_seconds = config.initial_buffer_kb as f64 / 24.0;
        let minimum_buffer_seconds = config.minimum_buffer_kb as f64 / 24.0;

        assert!(initial_buffer_seconds >= 5.0, "Initial buffer should be at least 5 seconds");
        assert!(minimum_buffer_seconds >= 3.0, "Minimum buffer should be at least 3 seconds");
        assert!(config.initial_buffer_kb > config.minimum_buffer_kb, "Initial buffer should be larger than minimum");
    }

    #[test]
    fn test_config_stream_rate_multiplier() {
        env::set_var("STREAM_RATE_MULTIPLIER", "1.05");
        let config = Config::from_env();
        assert_eq!(config.stream_rate_multiplier, 1.05);
        env::remove_var("STREAM_RATE_MULTIPLIER");

        env::set_var("STREAM_RATE_MULTIPLIER", "1.20");
        let config = Config::from_env();
        assert_eq!(config.stream_rate_multiplier, 1.20);
        env::remove_var("STREAM_RATE_MULTIPLIER");
    }
}
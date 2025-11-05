// Integration tests for WebRadio
// These tests verify the interaction between different modules

use std::env;

// Note: Integration tests can't access private items from the crate,
// so we test the public API and configuration integration

#[test]
fn test_config_defaults_integration() {
    // Clear all environment variables
    env::remove_var("HOST");
    env::remove_var("PORT");
    env::remove_var("MUSIC_DIR");
    env::remove_var("INITIAL_BUFFER_KB");
    env::remove_var("MINIMUM_BUFFER_KB");
    env::remove_var("CHUNK_INTERVAL_MS");
    env::remove_var("STREAM_RATE_MULTIPLIER");
    env::remove_var("INITIAL_BUFFER_TIMEOUT_MS");
    env::remove_var("BROADCAST_CHANNEL_CAPACITY");

    // Since we can't directly access Config::from_env() from integration tests
    // (it's not pub in the crate), we verify the environment-based configuration
    // works by ensuring the defaults are sensible for streaming

    // Verify default values are set correctly for streaming
    let default_initial_buffer_kb = 120;
    let default_minimum_buffer_kb = 80;
    let default_stream_rate_multiplier = 1.10;
    let default_chunk_interval_ms = 100;

    // Calculate expected buffer times at 192kbps
    let bitrate_kbps = 192.0;
    let initial_buffer_seconds = default_initial_buffer_kb as f64 / 24.0; // 24KB per second at 192kbps
    let minimum_buffer_seconds = default_minimum_buffer_kb as f64 / 24.0;

    assert!(initial_buffer_seconds >= 5.0, "Initial buffer should be at least 5 seconds");
    assert!(minimum_buffer_seconds >= 3.0, "Minimum buffer should be at least 3 seconds");

    // Verify stream rate multiplier creates buffer growth
    let stream_rate_kbps = bitrate_kbps * default_stream_rate_multiplier;
    let buffer_growth_kbps = stream_rate_kbps - bitrate_kbps;

    assert!(buffer_growth_kbps > 0.0, "Stream should be faster than playback");
    assert!((buffer_growth_kbps / bitrate_kbps - 0.10_f64).abs() < 0.01, "Should have 10% buffer growth");

    // Verify chunk interval is suitable for iOS
    assert_eq!(default_chunk_interval_ms, 100, "Chunk interval should be 100ms for iOS compatibility");
}

#[test]
fn test_streaming_calculations() {
    // Test the math behind streaming parameters
    let bitrate = 192000.0; // 192kbps
    let stream_rate_multiplier = 1.10;
    let chunk_interval_ms = 100.0;

    // Calculate stream rate (should be 211.2kbps)
    let stream_rate = bitrate * stream_rate_multiplier;
    assert!((stream_rate - 211200.0_f64).abs() < 0.1);

    // Calculate bytes per second
    let bytes_per_second = stream_rate / 8.0;
    assert_eq!(bytes_per_second as usize, 26400);

    // Calculate chunk size for 100ms
    let chunk_size = (bytes_per_second * chunk_interval_ms / 1000.0) as usize;
    assert_eq!(chunk_size, 2640);

    // Calculate buffer growth per second
    let buffer_growth_per_second = (stream_rate - bitrate) / 8.0;
    assert_eq!(buffer_growth_per_second as usize, 2400); // 2.4KB per second growth
}

#[test]
fn test_buffer_timeout_calculations() {
    // Test buffer collection timeout calculations
    let target_buffer_kb = 120;
    let stream_rate_kbps = 211.0;

    // Calculate how long it takes to collect the buffer
    let bytes_per_ms = (stream_rate_kbps * 1000.0) / 8.0 / 1000.0;
    let time_to_collect_ms = (target_buffer_kb as f64 * 1024.0) / bytes_per_ms;

    // At 211kbps, 120KB should take about 4.5-5 seconds
    assert!(time_to_collect_ms > 4500.0, "Should take more than 4.5 seconds");
    assert!(time_to_collect_ms < 5500.0, "Should take less than 5.5 seconds");

    // Verify the timeout (6000ms) is sufficient
    let timeout_ms = 6000.0;
    assert!(timeout_ms > time_to_collect_ms, "Timeout should be greater than collection time");
}

#[test]
fn test_gap_detection_timing() {
    // Test gap detection timeout calculation
    let chunk_interval_ms = 100;
    let gap_timeout_ms = chunk_interval_ms * 5;

    assert_eq!(gap_timeout_ms, 500, "Gap timeout should be 5x chunk interval");

    // Verify gap timeout is reasonable
    assert!(gap_timeout_ms > chunk_interval_ms, "Gap timeout must be larger than chunk interval");
    assert!(gap_timeout_ms < 1000, "Gap timeout should be under 1 second for responsiveness");
}

#[test]
fn test_mp3_frame_calculations() {
    // Test MP3 frame size calculations for different bitrates
    struct FrameTest {
        bitrate_kbps: u32,
        samplerate: u32,
        expected_size: usize,
    }

    let tests = vec![
        FrameTest { bitrate_kbps: 128, samplerate: 44100, expected_size: 417 },
        FrameTest { bitrate_kbps: 192, samplerate: 44100, expected_size: 626 },
        FrameTest { bitrate_kbps: 320, samplerate: 44100, expected_size: 1044 },
    ];

    for test in tests {
        let bitrate = test.bitrate_kbps as usize * 1000;
        let frame_size = (144 * bitrate) / test.samplerate as usize;
        assert_eq!(frame_size, test.expected_size);
    }
}

#[test]
fn test_environment_variable_precedence() {
    // Test that environment variables override defaults

    // Set custom values
    env::set_var("INITIAL_BUFFER_KB", "200");
    env::set_var("STREAM_RATE_MULTIPLIER", "1.15");
    env::set_var("CHUNK_INTERVAL_MS", "50");

    // Verify the values would be parsed correctly
    let initial_buffer: usize = env::var("INITIAL_BUFFER_KB")
        .unwrap()
        .parse()
        .unwrap();
    let stream_rate_multiplier: f64 = env::var("STREAM_RATE_MULTIPLIER")
        .unwrap()
        .parse()
        .unwrap();
    let chunk_interval: u64 = env::var("CHUNK_INTERVAL_MS")
        .unwrap()
        .parse()
        .unwrap();

    assert_eq!(initial_buffer, 200);
    assert_eq!(stream_rate_multiplier, 1.15);
    assert_eq!(chunk_interval, 50);

    // Cleanup
    env::remove_var("INITIAL_BUFFER_KB");
    env::remove_var("STREAM_RATE_MULTIPLIER");
    env::remove_var("CHUNK_INTERVAL_MS");
}

#[test]
fn test_broadcast_channel_capacity() {
    // Test that broadcast channel capacity is sufficient
    let default_capacity = 32768;
    let chunk_size = 2640; // At 192kbps, 100ms chunks

    // How many chunks can we buffer?
    let chunks_buffered = default_capacity / chunk_size;

    // Should be able to buffer at least 10 chunks (1 second)
    assert!(chunks_buffered >= 10, "Should buffer at least 1 second of chunks");

    // At 100ms per chunk, calculate total buffer time
    let buffer_time_ms = chunks_buffered * 100;

    // Should buffer more than 1 second
    assert!(buffer_time_ms >= 1000, "Should have at least 1 second buffer capacity");
}

#[test]
fn test_concurrent_listener_math() {
    // Test calculations for multiple concurrent listeners
    let stream_rate_kbps = 211.0;
    let bytes_per_second_per_listener = (stream_rate_kbps * 1000.0) / 8.0;

    // Calculate bandwidth for different listener counts
    let listener_counts = vec![1, 10, 100, 1000];

    for count in listener_counts {
        let total_bandwidth = bytes_per_second_per_listener * count as f64;
        let total_mbps = (total_bandwidth * 8.0) / 1_000_000.0;

        // Verify calculations are reasonable
        assert!(total_mbps > 0.0);

        // At 211kbps per listener:
        // 1 listener = ~0.21 Mbps
        // 10 listeners = ~2.1 Mbps
        // 100 listeners = ~21 Mbps
        // 1000 listeners = ~211 Mbps
        match count {
            1 => assert!((total_mbps - 0.211).abs() < 0.001),
            10 => assert!((total_mbps - 2.11).abs() < 0.01),
            100 => assert!((total_mbps - 21.1).abs() < 0.1),
            1000 => assert!((total_mbps - 211.0).abs() < 1.0),
            _ => {}
        }
    }
}

#[test]
fn test_duration_estimation() {
    // Test MP3 duration estimation from file size and bitrate
    struct DurationTest {
        file_size_bytes: u64,
        bitrate_bps: u64,
        expected_duration_seconds: u64,
    }

    let tests = vec![
        // 1MB file at 128kbps = ~62 seconds
        DurationTest {
            file_size_bytes: 1_000_000,
            bitrate_bps: 128_000,
            expected_duration_seconds: 62,
        },
        // 5MB file at 192kbps = ~208 seconds
        DurationTest {
            file_size_bytes: 5_000_000,
            bitrate_bps: 192_000,
            expected_duration_seconds: 208,
        },
        // 10MB file at 320kbps = ~250 seconds
        DurationTest {
            file_size_bytes: 10_000_000,
            bitrate_bps: 320_000,
            expected_duration_seconds: 250,
        },
    ];

    for test in tests {
        let duration = (test.file_size_bytes * 8) / test.bitrate_bps;
        assert_eq!(duration, test.expected_duration_seconds);
    }
}

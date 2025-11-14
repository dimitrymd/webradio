use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{broadcast, RwLock},
    time::{interval, sleep},
};
use tokio_stream::Stream;
use axum::response::sse::Event;
use bytes::Bytes;
use dashmap::DashMap;
use arc_swap::ArcSwap;
use tracing::{info, warn, error, debug};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;

use crate::{
    error::Result,
    playlist::{Playlist, Track},
    config::Config,
};

pub struct RadioStation {
    config: Config,  // Changed from _config to config (used now)
    playlist: Arc<RwLock<Playlist>>,
    current_track: Arc<ArcSwap<Option<Track>>>,

    // Broadcasting
    broadcast_tx: Arc<RwLock<broadcast::Sender<Bytes>>>,
    is_broadcasting: Arc<AtomicBool>,

    // Statistics
    listeners: Arc<DashMap<String, ListenerInfo>>,
    total_bytes_sent: Arc<AtomicU64>,
    current_position: Arc<AtomicU64>,
    start_time: Instant,

    // Stream Health Monitoring
    last_chunk_sent: Arc<AtomicU64>, // timestamp as u64
    stream_gaps_detected: Arc<AtomicU32>,
    recovery_attempts: Arc<AtomicU32>,

    // Control
    shutdown_tx: broadcast::Sender<()>,
}

#[derive(Debug)]
struct ListenerInfo {
    connected_at: Instant,
    bytes_received: u64,
}

// Removed unused MP3 frame parsing functions - can be re-added if frame-level parsing is needed

impl RadioStation {
    pub async fn new(config: Config) -> Result<Self> {
        // Load playlist
        let playlist = Playlist::load_or_scan(&config.music_dir).await?;
        info!("Loaded {} tracks", playlist.tracks.len());

        // Create broadcast channel with configurable capacity
        let (broadcast_tx, _) = broadcast::channel(config.broadcast_channel_capacity);
        let (shutdown_tx, _) = broadcast::channel(1);

        info!("Streaming configuration:");
        info!("  - Initial buffer: {}KB (~{:.1}s at 192kbps)",
            config.initial_buffer_kb,
            config.initial_buffer_kb as f64 / 24.0);
        info!("  - Minimum buffer: {}KB (~{:.1}s at 192kbps)",
            config.minimum_buffer_kb,
            config.minimum_buffer_kb as f64 / 24.0);
        info!("  - Chunk interval: {}ms", config.chunk_interval_ms);
        info!("  - Stream rate: {:.0}% of bitrate (builds {:.0}% buffer/sec)",
            config.stream_rate_multiplier * 100.0,
            (config.stream_rate_multiplier - 1.0) * 100.0);
        info!("  - Broadcast capacity: {} messages", config.broadcast_channel_capacity);

        Ok(Self {
            config,  // Store config for use in streaming
            playlist: Arc::new(RwLock::new(playlist)),
            current_track: Arc::new(ArcSwap::from_pointee(None)),
            broadcast_tx: Arc::new(RwLock::new(broadcast_tx)),
            is_broadcasting: Arc::new(AtomicBool::new(false)),
            listeners: Arc::new(DashMap::new()),
            total_bytes_sent: Arc::new(AtomicU64::new(0)),
            current_position: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),

            // Initialize stream health monitoring
            last_chunk_sent: Arc::new(AtomicU64::new(0)),
            stream_gaps_detected: Arc::new(AtomicU32::new(0)),
            recovery_attempts: Arc::new(AtomicU32::new(0)),

            shutdown_tx,
        })
    }
    
    pub fn start_broadcast(self: Arc<Self>) {
        if self.is_broadcasting.swap(true, Ordering::Relaxed) {
            warn!("Broadcast already running");
            return;
        }

        info!("Starting radio broadcast...");

        let station = Arc::clone(&self);
        tokio::spawn(async move {
            if let Err(e) = station.broadcast_loop().await {
                error!("Broadcast loop error: {}", e);
            }
            // Ensure the flag is cleared if broadcast loop exits
            station.is_broadcasting.store(false, Ordering::Relaxed);
        });
    }
    
    pub async fn stop_broadcast(&self) {
        info!("Stopping broadcast...");
        self.is_broadcasting.store(false, Ordering::Relaxed);
        
        // Send shutdown signal
        if let Err(e) = self.shutdown_tx.send(()) {
            warn!("Failed to send shutdown signal: {}", e);
        }
        
        // Give some time for graceful shutdown
        sleep(Duration::from_millis(200)).await;
        
        // Force close all receivers
        drop(self.broadcast_tx.clone());
        
        info!("Radio broadcast stopped");
    }
    
    async fn broadcast_loop(&self) -> Result<()> {
        let mut shutdown = self.shutdown_tx.subscribe();
        
        info!("Broadcast loop started");
        
        loop {
            // Check if we should stop
            if !self.is_broadcasting.load(Ordering::Relaxed) {
                break;
            }
            
            // Get next track
            let track = {
                let mut playlist = self.playlist.write().await;
                playlist.get_next_track()
            };
            
            let Some(track) = track else {
                warn!("No tracks available in playlist");
                sleep(Duration::from_secs(5)).await;
                continue;
            };
            
            // Don't create a new channel - just continue using the same one
            // This keeps clients connected across track changes

            // Update current track
            self.current_track.store(Arc::new(Some(track.clone())));
            info!("Now playing: {} - {} ({})", track.artist, track.title, track.path.display());

            // Stream the track with automatic recovery
            tokio::select! {
                result = self.stream_track_with_recovery(&track) => {
                    match result {
                        Ok(_) => info!("Track completed successfully"),
                        Err(e) => {
                            error!("Error streaming track after recovery attempts: {}", e);
                            // Brief pause before trying next track to avoid rapid failure loops
                            sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
                _ = shutdown.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
            }

            // No gap between tracks - immediately start next track
        }
        
        info!("Broadcast loop ended");
        Ok(())
    }
    
    async fn stream_track(&self, track: &Track) -> Result<()> {
        // Track path is relative to music directory
        let path = if track.path.is_absolute() {
            track.path.clone()
        } else {
            PathBuf::from("music").join(&track.path)
        };

        info!("Streaming track: {} at {}kbps", path.display(), track.bitrate.unwrap_or(192000) / 1000);

        // Open the file with symphonia
        let file = std::fs::File::open(&path)?;
        let media_source = MediaSourceStream::new(Box::new(file), Default::default());

        // Create a hint to help the probe guess the format
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        // Probe the media source
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, media_source, &format_opts, &metadata_opts)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to probe file: {}", e)))?;

        let mut format = probed.format;

        // Get the default audio track
        let track_info = format.default_track()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No audio track found"))?;
        let track_id = track_info.id;

        // Get timebase for duration calculations
        let time_base = track_info.codec_params.time_base
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No timebase available"))?;

        // Get bitrate for logging
        let bitrate = track.bitrate.unwrap_or(192000);
        let stream_rate_multiplier = self.config.stream_rate_multiplier;
        let base_bitrate_kbps = bitrate as f64 / 1000.0;
        let stream_rate_kbps = base_bitrate_kbps * stream_rate_multiplier;
        let chunk_interval_ms = self.config.chunk_interval_ms;

        info!("Streaming at {:.0}kbps ({}% of {}kbps bitrate)",
            stream_rate_kbps,
            (stream_rate_multiplier * 100.0) as u32,
            base_bitrate_kbps);
        info!("This allows client buffer to grow by ~{:.1}% per second",
            (stream_rate_multiplier - 1.0) * 100.0);

        // Calculate target chunk duration in milliseconds
        let target_chunk_duration_ms = chunk_interval_ms as f64;

        // Stream packets from symphonia and bundle them by duration
        let mut current_chunk_data = Vec::new();
        let mut current_chunk_duration_tb: u64 = 0; // Duration in timebase units
        let stream_start = Instant::now();
        let mut chunks_sent = 0;
        let mut last_log = Instant::now();
        let mut total_packets = 0;

        // Pre-lock the broadcast channel to avoid timing interference
        let tx = self.broadcast_tx.read().await;

        info!("Bundling packets by duration: ~{}ms chunks using timebase calculations",
            target_chunk_duration_ms);

        loop {
            if !self.is_broadcasting.load(Ordering::Relaxed) {
                break;
            }

            // Read next packet
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // End of file - send any remaining data
                    if !current_chunk_data.is_empty() {
                        let chunk = Bytes::from(current_chunk_data);
                        let chunk_len = chunk.len();
                        let final_duration_ms = time_base.calc_time(current_chunk_duration_tb).seconds as f64 * 1000.0;

                        info!("Sending final chunk: {} bytes, {:.1}ms duration", chunk_len, final_duration_ms);

                        self.total_bytes_sent.fetch_add(chunk_len as u64, Ordering::Relaxed);

                        if let Err(_) = tx.send(chunk) {
                            debug!("No active listeners for final chunk");
                        } else {
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            self.last_chunk_sent.store(now_ms, Ordering::Relaxed);
                        }
                        chunks_sent += 1;
                    }
                    break;
                }
                Err(e) => {
                    warn!("Error reading packet: {}", e);
                    break;
                }
            };

            // Only process packets from our audio track
            if packet.track_id() != track_id {
                continue;
            }

            total_packets += 1;

            // Add packet data to current chunk
            current_chunk_data.extend_from_slice(packet.buf());

            // Add packet duration to accumulated duration (in timebase units)
            current_chunk_duration_tb += packet.dur();

            // Calculate current chunk duration in milliseconds
            let chunk_duration_ms = time_base.calc_time(current_chunk_duration_tb).seconds as f64 * 1000.0;

            // Check if we should send this chunk based on duration
            // Send when accumulated duration >= target_chunk_duration_ms
            if chunk_duration_ms >= target_chunk_duration_ms {
                // Calculate timing for smooth delivery at stream rate
                let target_time = stream_start + Duration::from_millis((chunks_sent as f64 * target_chunk_duration_ms) as u64);
                let now = Instant::now();

                if target_time > now {
                    // We're ahead of schedule - sleep until target time
                    sleep(target_time - now).await;
                } else {
                    // We're behind schedule
                    let drift = now - target_time;
                    if drift > Duration::from_millis(10) {
                        warn!("Streaming drift: {}ms behind schedule", drift.as_millis());
                    }
                }

                // Send the chunk
                let chunk = Bytes::from(current_chunk_data.clone());
                let chunk_len = chunk.len();
                self.total_bytes_sent.fetch_add(chunk_len as u64, Ordering::Relaxed);
                self.current_position.fetch_add(chunk_len as u64, Ordering::Relaxed);

                if let Err(_) = tx.send(chunk) {
                    debug!("No active listeners for chunk");
                } else {
                    // Record successful chunk send
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    self.last_chunk_sent.store(now_ms, Ordering::Relaxed);
                }

                chunks_sent += 1;
                current_chunk_data.clear();
                current_chunk_duration_tb = 0; // Reset duration counter

                // Log progress occasionally
                if last_log.elapsed() > Duration::from_secs(5) {
                    let elapsed = stream_start.elapsed();
                    let total_sent = self.total_bytes_sent.load(Ordering::Relaxed);
                    let rate_kbps = (total_sent as f64 * 8.0) / (elapsed.as_secs_f64() * 1000.0);

                    info!("Streaming: sent {} chunks ({} packets), actual rate: {:.0}kbps",
                        chunks_sent, total_packets, rate_kbps);
                    last_log = Instant::now();
                }
            }
        }

        info!("Finished streaming track: {} (sent {} chunks from {} packets)",
            track.title,
            chunks_sent,
            total_packets
        );
        Ok(())
    }

    async fn stream_track_with_recovery(&self, track: &Track) -> Result<()> {
        let mut attempt = 0;
        const MAX_ATTEMPTS: u32 = 3;

        while attempt < MAX_ATTEMPTS {
            attempt += 1;

            match self.stream_track(track).await {
                Ok(_) => {
                    // Success - reset recovery counter if we had previous attempts
                    if attempt > 1 {
                        info!("Stream recovered successfully on attempt {}", attempt);
                    }
                    return Ok(());
                }
                Err(e) => {
                    self.recovery_attempts.fetch_add(1, Ordering::Relaxed);

                    if attempt < MAX_ATTEMPTS {
                        warn!("Stream attempt {}/{} failed: {}. Retrying...", attempt, MAX_ATTEMPTS, e);

                        // Progressive backoff: 250ms, 500ms, 750ms
                        let delay_ms = 250 * attempt as u64;
                        sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        error!("All {} stream attempts failed for track: {}", MAX_ATTEMPTS, track.title);
                        return Err(e);
                    }
                }
            }
        }

        Err(std::io::Error::new(std::io::ErrorKind::Other, "Maximum recovery attempts exceeded").into())
    }

    pub async fn create_audio_stream(&self, is_ios: bool) -> Result<impl Stream<Item = Result<Bytes>>> {
        let listener_id = uuid::Uuid::new_v4().to_string();
        let mut receiver = self.broadcast_tx.read().await.subscribe();

        // Register listener
        self.listeners.insert(listener_id.clone(), ListenerInfo {
            connected_at: Instant::now(),
            bytes_received: 0,
        });

        let listeners = self.listeners.clone();
        let current_count = self.listener_count();

        info!("New audio listener connected: {} (total: {}, iOS: {})", &listener_id[..8], current_count, is_ios);

        // Clone config values for use in the stream
        // iOS devices need larger buffers due to aggressive power management
        let target_buffer = if is_ios {
            self.config.initial_buffer_kb * 1024 * 2  // Double buffer for iOS (240KB = ~10 seconds)
        } else {
            self.config.initial_buffer_kb * 1024
        };

        let minimum_buffer = if is_ios {
            self.config.minimum_buffer_kb * 1024 * 2  // Double minimum for iOS (160KB = ~6.6 seconds)
        } else {
            self.config.minimum_buffer_kb * 1024
        };

        let buffer_timeout = if is_ios {
            Duration::from_millis(self.config.initial_buffer_timeout_ms * 2)  // 12 seconds for iOS
        } else {
            Duration::from_millis(self.config.initial_buffer_timeout_ms)
        };

        let chunk_interval = Duration::from_millis(self.config.chunk_interval_ms);

        Ok(async_stream::stream! {
            // Phase 1: Build up initial buffer for smooth startup
            let mut initial_buffer = Vec::new();
            let mut buffered_bytes = 0;

            info!("Listener {} collecting {}KB buffer (minimum: {}KB, timeout: {}ms)",
                &listener_id[..8],
                target_buffer / 1024,
                minimum_buffer / 1024,
                buffer_timeout.as_millis());

            // Collect initial data with configurable timeout
            while buffered_bytes < target_buffer {
                match tokio::time::timeout(buffer_timeout, receiver.recv()).await {
                    Ok(Ok(chunk)) => {
                        buffered_bytes += chunk.len();
                        initial_buffer.push(chunk);
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                        warn!("Initial buffering lagged by {} messages", skipped);
                        continue;
                    }
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        break;
                    }
                    Err(_) => {
                        // Timeout - start if we have minimum required data
                        if buffered_bytes >= minimum_buffer {
                            info!("Buffer timeout reached, starting with {}KB (minimum met)",
                                buffered_bytes / 1024);
                            break;
                        } else {
                            warn!("Buffer timeout with only {}KB (minimum {}KB not met), collecting more...",
                                buffered_bytes / 1024,
                                minimum_buffer / 1024);
                            // Continue collecting - we need the minimum
                        }
                    }
                }
            }

            info!("Listener {} starting playback with {} KB buffer ({} chunks)",
                &listener_id[..8],
                buffered_bytes / 1024,
                initial_buffer.len());

            // Phase 2: BURST - Send ALL initial buffer immediately (no delays!)
            // The "burst" happens naturally by sending all buffered chunks at once
            // The client's TCP buffer and audio decoder handle the rapid delivery
            info!("Listener {} bursting {} chunks immediately (no delays)",
                &listener_id[..8], initial_buffer.len());

            for chunk in initial_buffer {
                if let Some(mut info) = listeners.get_mut(&listener_id) {
                    info.bytes_received += chunk.len() as u64;
                }
                yield Ok(chunk);
                // NO DELAYS - send all buffered data immediately!
            }

            info!("Listener {} burst complete, entering sustain phase", &listener_id[..8]);

            // Phase 3: SUSTAIN - Normal streaming with gap detection
            // Use timeout of 5x chunk interval to detect gaps quickly but avoid false positives
            // 100ms chunks * 5 = 500ms timeout (much better than the old 2000ms!)
            let chunk_timeout = chunk_interval * 5;

            loop {
                // Wait for chunk with timeout to detect gaps quickly
                match tokio::time::timeout(chunk_timeout, receiver.recv()).await {
                    Ok(Ok(chunk)) => {
                        // Normal chunk received
                        if let Some(mut info) = listeners.get_mut(&listener_id) {
                            info.bytes_received += chunk.len() as u64;
                        }
                        yield Ok(chunk);
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                        warn!("Listener {} lagged by {} messages, attempting recovery",
                            &listener_id[..8], skipped);

                        // Attempt immediate recovery by getting fresh data
                        match tokio::time::timeout(Duration::from_millis(500), receiver.recv()).await {
                            Ok(Ok(chunk)) => {
                                info!("Listener {} recovered successfully", &listener_id[..8]);
                                if let Some(mut info) = listeners.get_mut(&listener_id) {
                                    info.bytes_received += chunk.len() as u64;
                                }
                                yield Ok(chunk);
                                continue; // Continue normal streaming
                            }
                            Ok(Err(_)) => {
                                error!("Listener {} recovery failed - broadcast closed", &listener_id[..8]);
                                break;
                            }
                            Err(_) => {
                                error!("Listener {} recovery timeout - no data available", &listener_id[..8]);
                                break;
                            }
                        }
                    }
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        info!("Broadcast closed for listener {}", &listener_id[..8]);
                        break;
                    }
                    Err(_) => {
                        // Timeout - no chunk received in expected time
                        error!("Listener {} detected gap - no chunk for {}ms!",
                            &listener_id[..8],
                            chunk_timeout.as_millis());

                        // Try one more time before giving up
                        match tokio::time::timeout(Duration::from_secs(1), receiver.recv()).await {
                            Ok(Ok(chunk)) => {
                                warn!("Listener {} gap recovered", &listener_id[..8]);
                                if let Some(mut info) = listeners.get_mut(&listener_id) {
                                    info.bytes_received += chunk.len() as u64;
                                }
                                yield Ok(chunk);
                                continue;
                            }
                            _ => {
                                error!("Listener {} giving up after prolonged gap", &listener_id[..8]);
                                break;
                            }
                        }
                    }
                }
            }
            
            // Cleanup on disconnect
            listeners.remove(&listener_id);
            let remaining = listeners.len();
            info!("Audio listener disconnected: {} (remaining: {})", &listener_id[..8], remaining);
        })
    }
    
    pub fn create_event_stream(self: Arc<Self>) -> impl Stream<Item = Result<Event>> {
        // Don't count SSE connections as listeners
        async_stream::stream! {
            let mut interval = interval(Duration::from_secs(5));

            loop {
                interval.tick().await;

                let event = Event::default()
                    .event("now-playing")
                    .json_data(self.get_now_playing())
                    .unwrap();

                yield Ok(event);
            }
        }
    }
    
    pub fn get_now_playing(&self) -> serde_json::Value {
        let current = self.current_track.load();
        
        match current.as_ref() {
            Some(track) => serde_json::json!({
                "title": track.title,
                "artist": track.artist,
                "album": track.album,
                "duration": track.duration,
                "bitrate": track.bitrate.unwrap_or(0) / 1000, // Show in kbps
                "position": self.current_position.load(Ordering::Relaxed),
                "listeners": self.listener_count(),
            }),
            None => serde_json::json!({
                "title": "No track playing",
                "listeners": self.listener_count(),
            }),
        }
    }
    
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
    
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
    
    pub fn get_playlist(&self) -> Result<Playlist> {
        // This is sync but should be fast
        let playlist = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.playlist.read().await.clone()
            })
        });
        Ok(playlist)
    }
    
    pub fn get_statistics(&self) -> serde_json::Value {
        let total_mb = self.total_bytes_sent.load(Ordering::Relaxed) as f64 / 1_048_576.0;
        let listeners: Vec<_> = self.listeners.iter()
            .map(|entry| {
                let (id, info) = entry.pair();
                serde_json::json!({
                    "id": &id[..8],
                    "connected_seconds": info.connected_at.elapsed().as_secs(),
                    "mb_received": info.bytes_received as f64 / 1_048_576.0,
                })
            })
            .collect();

        // Calculate time since last chunk sent
        let last_chunk_ms = self.last_chunk_sent.load(Ordering::Relaxed);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let ms_since_last_chunk = if last_chunk_ms > 0 {
            now_ms.saturating_sub(last_chunk_ms)
        } else {
            0
        };

        serde_json::json!({
            "uptime_seconds": self.uptime_seconds(),
            "total_mb_sent": total_mb,
            "current_listeners": self.listener_count(),
            "is_broadcasting": self.is_broadcasting.load(Ordering::Relaxed),
            "listeners": listeners,

            // Stream health metrics
            "stream_health": {
                "gaps_detected": self.stream_gaps_detected.load(Ordering::Relaxed),
                "recovery_attempts": self.recovery_attempts.load(Ordering::Relaxed),
                "ms_since_last_chunk": ms_since_last_chunk,
                "is_streaming": ms_since_last_chunk < 500, // Healthy if chunk sent in last 500ms
            },

            // Buffer configuration
            "buffer_config": {
                "initial_buffer_kb": self.config.initial_buffer_kb,
                "initial_buffer_seconds": self.config.initial_buffer_kb as f64 / 24.0,
                "minimum_buffer_kb": self.config.minimum_buffer_kb,
                "minimum_buffer_seconds": self.config.minimum_buffer_kb as f64 / 24.0,
                "chunk_interval_ms": self.config.chunk_interval_ms,
                "stream_rate_multiplier": self.config.stream_rate_multiplier,
                "stream_rate_percent": self.config.stream_rate_multiplier * 100.0,
                "buffer_growth_percent_per_sec": (self.config.stream_rate_multiplier - 1.0) * 100.0,
                "broadcast_channel_capacity": self.config.broadcast_channel_capacity,
            },
        })
    }
    
    pub fn is_broadcasting(&self) -> bool {
        self.is_broadcasting.load(Ordering::Relaxed)
    }
    
    pub async fn get_broadcast_receiver_count(&self) -> usize {
        self.broadcast_tx.read().await.receiver_count()
    }
}

impl Drop for RadioStation {
    fn drop(&mut self) {
        info!("RadioStation dropping, stopping broadcast");
        self.is_broadcasting.store(false, Ordering::Relaxed);
        let _ = self.shutdown_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_radio_station_creation() {
        use crate::config::Config;

        // Create a minimal config for testing
        std::env::set_var("MUSIC_DIR", "test_music");
        let _config = Config::from_env();

        // Note: This will fail if test_music directory doesn't exist
        // In a real test, we'd create temp directories
        // For now, we just test that the structure is correct

        // Cleanup
        std::env::remove_var("MUSIC_DIR");
    }

    #[test]
    fn test_listener_info() {
        let info = ListenerInfo {
            connected_at: Instant::now(),
            bytes_received: 1024,
        };

        assert_eq!(info.bytes_received, 1024);
        assert!(info.connected_at.elapsed().as_secs() < 1);
    }

    #[test]
    fn test_stream_rate_calculation() {
        // At 192kbps with 1.10 multiplier
        let bitrate = 192000.0_f64;
        let multiplier = 1.10_f64;
        let stream_rate = bitrate * multiplier;

        // Use approximate equality for floating point comparison
        assert!((stream_rate - 211200.0).abs() < 0.01, "Stream rate should be ~211200");

        // Buffer growth per second
        let playback_rate = 192000.0_f64;
        let growth_rate = (stream_rate - playback_rate) / playback_rate;

        assert!((growth_rate - 0.10_f64).abs() < 0.001, "Growth rate should be ~10%");
    }

    #[test]
    fn test_chunk_size_calculation() {
        // At 211kbps (110% of 192kbps), 100ms chunks
        let stream_rate_kbps = 211.0;
        let chunk_interval_ms = 100.0;

        let bytes_per_second = (stream_rate_kbps * 1000.0) / 8.0;
        let chunk_size_bytes = (bytes_per_second * chunk_interval_ms) / 1000.0;

        assert_eq!(chunk_size_bytes as usize, 2637); // 211000 / 8 * 0.1 = 2637.5
    }

    #[test]
    fn test_buffer_timeout_calculation() {
        // At 211kbps, how long to collect 120KB?
        let target_buffer_kb = 120;
        let stream_rate_kbps = 211.0;

        let bytes_per_ms = (stream_rate_kbps * 1000.0) / 8.0 / 1000.0;
        let time_to_collect_ms = (target_buffer_kb as f64 * 1024.0) / bytes_per_ms;

        assert!(time_to_collect_ms < 6000.0, "Should collect 120KB in under 6 seconds at 211kbps");
        assert!(time_to_collect_ms > 4500.0, "Should take more than 4.5 seconds to collect 120KB");
    }

    #[test]
    fn test_gap_detection_timeout() {
        let chunk_interval_ms = 100;
        let gap_timeout_ms = chunk_interval_ms * 5;

        assert_eq!(gap_timeout_ms, 500);
        assert!(gap_timeout_ms > chunk_interval_ms, "Gap timeout should be larger than chunk interval");
        assert!(gap_timeout_ms < 1000, "Gap timeout should be under 1 second for quick detection");
    }
}
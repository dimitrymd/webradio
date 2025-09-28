use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{broadcast, RwLock},
    time::{interval, sleep},
};
use tokio_stream::Stream;
use axum::response::sse::Event;
use bytes::Bytes;
use dashmap::DashMap;
use arc_swap::ArcSwap;
use tracing::{info, warn, error, debug};

use crate::{
    error::Result,
    playlist::{Playlist, Track},
    config::Config,
};

// MP3 frame size calculation
fn calculate_mp3_frame_size(header: u32) -> Option<usize> {
    // Extract MPEG version (bits 19-20)
    let _version = match (header >> 19) & 0b11 {
        0b11 => 1, // MPEG 1
        0b10 => 2, // MPEG 2
        0b00 => 3, // MPEG 2.5
        _ => return None,
    };

    // Extract layer (bits 17-18)
    let layer = match (header >> 17) & 0b11 {
        0b01 => 3, // Layer III
        0b10 => 2, // Layer II
        0b11 => 1, // Layer I
        _ => return None,
    };

    // Only handle Layer III (MP3)
    if layer != 3 {
        return None;
    }

    // Extract bitrate index (bits 12-15)
    let bitrate_index = ((header >> 12) & 0xF) as usize;

    // Bitrate tables for MPEG1 Layer III
    let bitrates = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
    if bitrate_index == 0 || bitrate_index == 15 {
        return None;
    }
    let bitrate = bitrates[bitrate_index] * 1000;

    // Extract sample rate index (bits 10-11)
    let samplerate_index = ((header >> 10) & 0b11) as usize;
    let samplerates = [44100, 48000, 32000, 0];
    let samplerate = samplerates[samplerate_index];
    if samplerate == 0 {
        return None;
    }

    // Extract padding bit (bit 9)
    let padding = ((header >> 9) & 1) as usize;

    // Calculate frame size for MPEG1 Layer III
    // Frame size = (144 * bitrate) / samplerate + padding
    let frame_size = (144 * bitrate) / samplerate + padding;

    Some(frame_size)
}

pub struct RadioStation {
    _config: Config,
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

    // Track Preloading (prevent transition gaps)
    next_track_data: Arc<RwLock<Option<Vec<u8>>>>,
    next_track_info: Arc<RwLock<Option<Track>>>,

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
        
        // Create broadcast channel with much larger capacity to handle timing variations
        // At 192kbps, we send ~24KB/s. With 240-byte chunks, that's 100 messages/second
        // 5 minutes = 300 seconds = 30,000 messages. Use 32K to be safe.
        let (broadcast_tx, _) = broadcast::channel(32768);
        let (shutdown_tx, _) = broadcast::channel(1);
        
        Ok(Self {
            _config: config,
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

            // Initialize track preloading
            next_track_data: Arc::new(RwLock::new(None)),
            next_track_info: Arc::new(RwLock::new(None)),

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

        // Read entire file into memory for smooth streaming
        let mut file = File::open(&path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len();

        let mut file_data = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut file_data).await?;
        drop(file); // Close file immediately

        info!("Loaded {} KB of audio data into memory", file_data.len() / 1024);

        // Skip ID3v2 tag if present
        let mut position = 0usize;
        if file_data.len() > 10 && &file_data[..3] == b"ID3" {
            // Calculate ID3v2 tag size
            let size = ((file_data[6] as u32 & 0x7F) << 21)
                | ((file_data[7] as u32 & 0x7F) << 14)
                | ((file_data[8] as u32 & 0x7F) << 7)
                | (file_data[9] as u32 & 0x7F);

            position = 10 + size as usize;
            info!("Skipped ID3v2 tag of {} bytes", position);
        }

        // Get bitrate for timing
        let bitrate = track.bitrate.unwrap_or(192000);
        let bytes_per_second = bitrate as f64 / 8.0;

        info!("Streaming at {}kbps ({} bytes/second)", bitrate / 1000, bytes_per_second as u32);

        // Stream the track with accurate bitrate-based timing
        let _start_time = Instant::now();
        let mut last_log = Instant::now();

        // Find all MP3 frame boundaries for accurate streaming
        let mut frames = Vec::new();
        let mut scan_pos = position;

        while scan_pos < file_data.len() - 4 {
            // Look for MP3 frame sync (11 bits set)
            if file_data[scan_pos] == 0xFF && (file_data[scan_pos + 1] & 0xE0) == 0xE0 {
                // Valid frame header found
                let header = u32::from_be_bytes([
                    file_data[scan_pos],
                    file_data[scan_pos + 1],
                    file_data[scan_pos + 2],
                    file_data[scan_pos + 3],
                ]);

                // Calculate frame size
                if let Some(frame_size) = calculate_mp3_frame_size(header) {
                    frames.push((scan_pos, frame_size));
                    scan_pos += frame_size;
                } else {
                    scan_pos += 1;
                }
            } else {
                scan_pos += 1;
            }
        }

        info!("Found {} MP3 frames in track", frames.len());

        if frames.is_empty() {
            warn!("No valid MP3 frames found, falling back to raw streaming");
            return Ok(());
        }

        // Calculate frame duration based on actual bitrate
        let total_audio_bytes: usize = frames.iter().map(|(_, size)| size).sum();
        let duration_seconds = total_audio_bytes as f64 / bytes_per_second;
        let _ms_per_frame = (duration_seconds * 1000.0) / frames.len() as f64;

        // iOS-optimized streaming approach
        // Much smaller chunks for iOS Safari compatibility
        const STREAM_RATE_KBPS: f64 = 192.0;  // Exact target rate
        const CHUNK_SIZE_MS: f64 = 100.0;     // 100ms chunks for iOS compatibility

        let stream_bytes_per_second = (STREAM_RATE_KBPS * 1000.0) / 8.0;
        let chunk_size_bytes = ((stream_bytes_per_second * CHUNK_SIZE_MS) / 1000.0) as usize;
        let _chunk_interval = Duration::from_millis(CHUNK_SIZE_MS as u64);

        info!("Streaming at {:.0}kbps ({} byte chunks every {}ms)",
            STREAM_RATE_KBPS, chunk_size_bytes, CHUNK_SIZE_MS);

        let mut data_index = 0;
        let data_len = file_data.len();
        let stream_start = Instant::now();
        let mut chunks_sent = 0;

        while data_index < data_len {
            if !self.is_broadcasting.load(Ordering::Relaxed) {
                break;
            }

            // Precise real-time clock synchronization with microsecond precision
            let target_time = stream_start + Duration::from_millis(chunks_sent * CHUNK_SIZE_MS as u64);
            let now = Instant::now();

            if target_time > now {
                // We're ahead of schedule - sleep precisely
                let sleep_duration = target_time - now;

                // For very short sleeps, use spin-waiting for sub-millisecond precision
                if sleep_duration < Duration::from_micros(500) {
                    // Spin-wait for microsecond precision
                    while Instant::now() < target_time {
                        // Yield to prevent 100% CPU usage
                        tokio::task::yield_now().await;
                    }
                } else {
                    // Use sleep for longer durations
                    sleep(sleep_duration).await;
                }
            } else {
                // We're behind schedule - check how much
                let drift = now - target_time;
                if drift > Duration::from_millis(5) {
                    warn!("Streaming drift: {}ms behind schedule", drift.as_millis());
                }
                // Don't sleep if we're behind - catch up by sending immediately
            }

            // Prepare chunk
            let chunk_end = (data_index + chunk_size_bytes).min(data_len);
            let chunk_data = file_data[data_index..chunk_end].to_vec();

            if !chunk_data.is_empty() {
                let chunk = Bytes::from(chunk_data);
                self.total_bytes_sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                self.current_position.store(data_index as u64, Ordering::Relaxed);

                // Use try_read to avoid blocking - critical fix for streaming gaps
                match self.broadcast_tx.try_read() {
                    Ok(tx) => {
                        if let Err(_) = tx.send(chunk) {
                            // No active listeners - not an error, just debug info
                            debug!("No active listeners for chunk");
                        } else {
                            // Record successful chunk send for health monitoring
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            self.last_chunk_sent.store(now_ms, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        // Channel locked - don't block streaming, just warn and count gap
                        warn!("Broadcast channel locked, continuing stream");
                        self.stream_gaps_detected.fetch_add(1, Ordering::Relaxed);
                        // Continue to maintain timing - don't break the stream
                    }
                }
            }

            data_index = chunk_end;
            chunks_sent += 1;

            // Log progress occasionally
            if last_log.elapsed() > Duration::from_secs(5) {
                let progress = (data_index as f64 / data_len as f64) * 100.0;
                let elapsed = stream_start.elapsed();
                let rate_kbps = (data_index as f64 * 8.0) / (elapsed.as_secs_f64() * 1000.0);

                info!("Streaming: {:.1}% complete, rate: {:.0}kbps", progress, rate_kbps);
                last_log = Instant::now();
            }
        }

        info!("Finished streaming track: {} (sent {} chunks)",
            track.title,
            chunks_sent
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

    pub async fn create_audio_stream(&self) -> Result<impl Stream<Item = Result<Bytes>>> {
        let listener_id = uuid::Uuid::new_v4().to_string();
        let mut receiver = self.broadcast_tx.read().await.subscribe();

        // Register listener
        self.listeners.insert(listener_id.clone(), ListenerInfo {
            connected_at: Instant::now(),
            bytes_received: 0,
        });

        let listeners = self.listeners.clone();
        let current_count = self.listener_count();

        info!("New audio listener connected: {} (total: {})", &listener_id[..8], current_count);

        Ok(async_stream::stream! {
            // Build up initial buffer for smooth startup
            let mut initial_buffer = Vec::new();
            let mut buffered_bytes = 0;
            const TARGET_BUFFER: usize = 12 * 1024; // 12KB buffer for iOS

            // Collect initial data with longer timeout for chunk-based streaming
            while buffered_bytes < TARGET_BUFFER {
                match tokio::time::timeout(Duration::from_millis(600), receiver.recv()).await {
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
                        // Timeout - start if we have reasonable data
                        if buffered_bytes >= 4 * 1024 { // At least 4KB
                            break;
                        }
                    }
                }
            }

            info!("Listener {} starting with {} KB buffer", &listener_id[..8], buffered_bytes / 1024);

            // Send buffered chunks
            for chunk in initial_buffer {
                if let Some(mut info) = listeners.get_mut(&listener_id) {
                    info.bytes_received += chunk.len() as u64;
                }
                yield Ok(chunk);
            }

            // Continue with normal streaming
            loop {
                match receiver.recv().await {
                    Ok(chunk) => {
                        // Update listener stats
                        if let Some(mut info) = listeners.get_mut(&listener_id) {
                            info.bytes_received += chunk.len() as u64;
                        }
                        yield Ok(chunk);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("Listener {} lagged by {} messages, catching up", &listener_id[..8], skipped);
                        // Just continue - the lagged messages are already skipped
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Broadcast closed for listener {}", &listener_id[..8]);
                        break;
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
        
        serde_json::json!({
            "uptime_seconds": self.uptime_seconds(),
            "total_mb_sent": total_mb,
            "current_listeners": self.listener_count(),
            "is_broadcasting": self.is_broadcasting.load(Ordering::Relaxed),
            "listeners": listeners,
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
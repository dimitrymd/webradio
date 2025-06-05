// src/services/streamer.rs - Async I/O with memory caching and CPU optimizations

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use log::{info, error};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::path::PathBuf;
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;
use tokio::time::sleep;
use bytes::Bytes;

// Balanced chunk size for smooth playback without buffering
const BROADCAST_CHUNK_SIZE: usize = 16384;   // 16KB chunks for smoother playback
const BROADCAST_BUFFER_SIZE: usize = 200;    // Large buffer to prevent underruns
const INITIAL_BURST_CHUNKS: usize = 5;       // Send 5 chunks immediately

// Cached track data
#[derive(Clone)]
struct CachedTrack {
    data: Arc<Vec<u8>>,
    audio_start_offset: usize,  // Skip ID3 tags
    bitrate: u64,
    duration: u64,
}

#[derive(Clone)]
pub struct AudioChunk {
    pub data: Bytes,
    pub position: u64,
    pub timestamp: Instant,
    pub chunk_id: u64,
}

#[derive(Debug, Clone)]
struct ConnectionInfo {
    connected_at: Instant,
    last_heartbeat: Instant,
    platform: String,
}

#[derive(Clone)]
pub struct TrackState {
    pub position_seconds: u64,
    pub position_milliseconds: u64,  
    pub duration: u64,
    pub remaining_time: u64,
    pub is_near_end: bool,
    pub bitrate: u64,
    pub track_info: Option<String>,
}

pub struct StreamManager {
    is_streaming: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    
    track_position_ms: Arc<AtomicU64>,
    track_duration_secs: Arc<AtomicU64>,
    track_bitrate: Arc<AtomicU64>,
    
    current_track_json: Arc<RwLock<Option<String>>>,
    track_start_time: Arc<RwLock<Instant>>,
    
    music_folder: PathBuf,
    
    // Track cache
    cached_track: Arc<RwLock<Option<CachedTrack>>>,
    
    // Async runtime handle
    runtime_handle: tokio::runtime::Handle,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing Async I/O StreamManager with memory caching");
        
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE);
        
        Self {
            is_streaming: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            
            broadcast_tx: Arc::new(broadcast_tx),
            connections: Arc::new(RwLock::new(HashMap::new())),
            
            track_position_ms: Arc::new(AtomicU64::new(0)),
            track_duration_secs: Arc::new(AtomicU64::new(0)),
            track_bitrate: Arc::new(AtomicU64::new(192000)),
            
            current_track_json: Arc::new(RwLock::new(None)),
            track_start_time: Arc::new(RwLock::new(Instant::now())),
            
            music_folder: music_folder.to_path_buf(),
            
            cached_track: Arc::new(RwLock::new(None)),
            
            runtime_handle: tokio::runtime::Handle::current(),
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        if self.is_streaming.load(Ordering::Relaxed) {
            return;
        }
        
        let music_folder = self.music_folder.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let is_streaming = self.is_streaming.clone();
        let should_stop = self.should_stop.clone();
        
        let current_track_json = self.current_track_json.clone();
        let track_start_time = self.track_start_time.clone();
        let track_position_ms = self.track_position_ms.clone();
        let track_duration_secs = self.track_duration_secs.clone();
        let track_bitrate = self.track_bitrate.clone();
        let cached_track = self.cached_track.clone();
        
        // Spawn async task instead of thread
        self.runtime_handle.spawn(async move {
            Self::async_broadcast_loop(
                music_folder,
                broadcast_tx,
                is_streaming,
                should_stop,
                current_track_json,
                track_start_time,
                track_position_ms,
                track_duration_secs,
                track_bitrate,
                cached_track,
            ).await;
        });
    }
    
    async fn async_broadcast_loop(
        music_folder: PathBuf,
        broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
        is_streaming: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        current_track_json: Arc<RwLock<Option<String>>>,
        track_start_time: Arc<RwLock<Instant>>,
        track_position_ms: Arc<AtomicU64>,
        track_duration_secs: Arc<AtomicU64>,
        track_bitrate: Arc<AtomicU64>,
        cached_track: Arc<RwLock<Option<CachedTrack>>>,
    ) {
        info!("Async broadcast loop started with memory caching");
        is_streaming.store(true, Ordering::Relaxed);
        
        let mut cached_playlist = None;
        let mut last_playlist_check = Instant::now();
        
        'main: loop {
            if should_stop.load(Ordering::Relaxed) {
                break 'main;
            }
            
            // When no listeners, use async sleep
            if broadcast_tx.receiver_count() == 0 {
                // Update virtual position
                let start_time = track_start_time.read().clone();
                let elapsed = start_time.elapsed().as_millis() as u64;
                track_position_ms.store(elapsed, Ordering::Relaxed);
                
                // Clear cache when no listeners
                if cached_track.read().is_some() {
                    info!("No listeners - clearing track cache to free memory");
                    *cached_track.write() = None;
                }
                
                // Longer sleep when no listeners
                sleep(Duration::from_secs(30)).await;
                continue;
            }
            
            // Get playlist (with caching)
            let playlist = if last_playlist_check.elapsed() > Duration::from_secs(120) || cached_playlist.is_none() {
                last_playlist_check = Instant::now();
                let p = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
                cached_playlist = Some(p.clone());
                p
            } else {
                cached_playlist.as_ref().unwrap().clone()
            };
            
            if playlist.tracks.is_empty() {
                sleep(Duration::from_secs(30)).await;
                continue;
            }
            
            let track_index = playlist.current_track % playlist.tracks.len();
            let track = &playlist.tracks[track_index];
            let track_path = music_folder.join(&track.path);
            
            // Check if file exists
            let file_exists = tokio::fs::metadata(&track_path).await.is_ok();
            if !file_exists {
                Self::advance_to_next_track(&playlist, track_index);
                cached_playlist = None;
                sleep(Duration::from_secs(1)).await;
                continue;
            }
            
            // Load track into memory if not already cached
            let cached = match Self::load_track_to_memory(&track_path).await {
                Ok(cached) => {
                    let file_size_mb = cached.data.len() as f64 / (1024.0 * 1024.0);
                    info!("Loaded track into memory: \"{}\" - {:.1}MB", track.title, file_size_mb);
                    
                    // Store in cache
                    *cached_track.write() = Some(cached.clone());
                    cached
                },
                Err(e) => {
                    error!("Failed to load track into memory: {}", e);
                    Self::advance_to_next_track(&playlist, track_index);
                    cached_playlist = None;
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            
            // Use pre-calculated values
            let actual_duration = track.duration.max(180);
            let actual_bitrate = cached.bitrate;
            
            // Update track info
            {
                *current_track_json.write() = serde_json::to_string(&track).ok();
                *track_start_time.write() = Instant::now();
            }
            
            track_duration_secs.store(actual_duration, Ordering::Relaxed);
            track_bitrate.store(actual_bitrate, Ordering::Relaxed);
            track_position_ms.store(0, Ordering::Relaxed);
            
            info!("Playing from memory: \"{}\" - {}s", track.title, actual_duration);
            
            // Stream the entire track (FIX: No loop needed here!)
            let stream_result = Self::stream_from_memory(
                &cached,
                &broadcast_tx,
                Duration::ZERO,  // Start from beginning
                actual_duration,
                &track_position_ms,
                &should_stop,
            ).await;
            
            match stream_result {
                Ok(_) => info!("Track streaming completed"),
                Err(e) => error!("Error streaming from memory: {}", e),
            }
            
            // Clear cache after track ends
            info!("Track finished - clearing memory cache");
            *cached_track.write() = None;
            
            // Advance to next track
            Self::advance_to_next_track(&playlist, track_index);
            cached_playlist = None;
            
            // Gap between tracks
            sleep(Duration::from_millis(500)).await;
            
            // Clear track info
            *current_track_json.write() = None;
            track_position_ms.store(0, Ordering::SeqCst);
        }
        
        is_streaming.store(false, Ordering::Relaxed);
    }
    
    async fn load_track_to_memory(track_path: &PathBuf) -> Result<CachedTrack, Box<dyn std::error::Error + Send + Sync>> {
        // Read entire file into memory
        let mut file = File::open(track_path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len() as usize;
        
        let mut data = Vec::with_capacity(file_size);
        file.read_to_end(&mut data).await?;
        
        // Find audio start (skip ID3 tags)
        let audio_start_offset = Self::find_audio_start(&data);
        
        // Estimate bitrate (default to 192kbps if unknown)
        let bitrate = 192000u64;
        
        // Calculate duration from file size
        let audio_size = (file_size - audio_start_offset) as u64;
        let duration = (audio_size * 8) / bitrate;
        
        Ok(CachedTrack {
            data: Arc::new(data),
            audio_start_offset,
            bitrate,
            duration,
        })
    }
    
    fn find_audio_start(data: &[u8]) -> usize {
        // Check for ID3v2 tag
        if data.len() > 10 && &data[0..3] == b"ID3" {
            // Calculate ID3v2 tag size
            let size = ((data[6] as usize & 0x7F) << 21)
                | ((data[7] as usize & 0x7F) << 14)
                | ((data[8] as usize & 0x7F) << 7)
                | (data[9] as usize & 0x7F);
            
            // ID3v2 header is 10 bytes + tag size
            let id3_end = 10 + size;
            
            // Find first MP3 frame after ID3
            for i in id3_end..data.len().saturating_sub(1) {
                // MP3 frame sync: 11 bits set to 1
                if data[i] == 0xFF && (data[i + 1] & 0xE0) == 0xE0 {
                    return i;
                }
            }
            
            // If no frame found, return end of ID3
            id3_end
        } else {
            // No ID3 tag, look for MP3 frame from start
            for i in 0..data.len().saturating_sub(1) {
                if data[i] == 0xFF && (data[i + 1] & 0xE0) == 0xE0 {
                    return i;
                }
            }
            0
        }
    }
    
    async fn stream_from_memory(
        cached: &CachedTrack,
        broadcast_tx: &Arc<broadcast::Sender<AudioChunk>>,
        start_elapsed: Duration,
        duration: u64,
        track_position_ms: &Arc<AtomicU64>,
        should_stop: &Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Calculate where we should be in the data
        let elapsed_ms = start_elapsed.as_millis() as u64;
        let bytes_per_ms = cached.bitrate / 8000;
        let target_position = cached.audio_start_offset + (elapsed_ms * bytes_per_ms) as usize;
        
        // Pre-calculate timing
        let chunk_duration_ms = (BROADCAST_CHUNK_SIZE as u64 * 8000) / cached.bitrate;
        let chunk_duration = Duration::from_millis(chunk_duration_ms);
        
        // Start timing
        let track_start = Instant::now() - start_elapsed;
        let mut current_position = target_position;
        let mut chunk_id = (target_position / BROADCAST_CHUNK_SIZE) as u64;
        let mut chunks_sent = 0u64;
        
        // Send a few chunks immediately to fill buffers
        let initial_chunks = 3;
        for _ in 0..initial_chunks {
            if current_position >= cached.data.len() {
                break;
            }
            
            let remaining = cached.data.len().saturating_sub(current_position);
            let chunk_size = remaining.min(BROADCAST_CHUNK_SIZE);
            let chunk_data = &cached.data[current_position..current_position + chunk_size];
            
            let chunk = AudioChunk {
                data: Bytes::copy_from_slice(chunk_data),
                position: current_position as u64,
                timestamp: Instant::now(),
                chunk_id,
            };
            
            let _ = broadcast_tx.send(chunk);
            current_position += chunk_size;
            chunk_id += 1;
            chunks_sent += 1;
        }
        
        // Now stream with proper timing
        let mut next_chunk_time = Instant::now() + chunk_duration;
        
        loop {
            // Check exit conditions first
            if should_stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            
            if broadcast_tx.receiver_count() == 0 {
                // No listeners, exit
                return Ok(());
            }
            
            // Check if track should end based on actual time
            let actual_elapsed = Instant::now().duration_since(track_start);
            if actual_elapsed >= Duration::from_secs(duration) {
                return Ok(());
            }
            
            // Check if we have enough data left
            let remaining = cached.data.len().saturating_sub(current_position);
            if remaining == 0 {
                return Ok(());
            }
            
            // Calculate how long to wait
            let now = Instant::now();
            if next_chunk_time > now {
                let wait_time = next_chunk_time - now;
                // Use a slightly shorter sleep to prevent drift
                if wait_time > Duration::from_millis(1) {
                    sleep(wait_time.saturating_sub(Duration::from_millis(1))).await;
                }
            }
            
            // Update next chunk time
            next_chunk_time = now + chunk_duration;
            
            // Update position occasionally
            chunks_sent += 1;
            if chunks_sent % 20 == 0 {
                let actual_pos_ms = actual_elapsed.as_millis() as u64;
                track_position_ms.store(actual_pos_ms, Ordering::Relaxed);
            }
            
            // Calculate chunk size
            let chunk_size = remaining.min(BROADCAST_CHUNK_SIZE);
            
            // Send chunk
            let chunk_data = &cached.data[current_position..current_position + chunk_size];
            
            let chunk = AudioChunk {
                data: Bytes::copy_from_slice(chunk_data),
                position: current_position as u64,
                timestamp: Instant::now(),
                chunk_id,
            };
            
            // Send without blocking - broadcast::send doesn't block
            let send_result = broadcast_tx.send(chunk);
            
            match send_result {
                Ok(_) => {
                    current_position += chunk_size;
                    chunk_id += 1;
                    
                    // If we've sent a partial chunk, we're at the end
                    if chunk_size < BROADCAST_CHUNK_SIZE {
                        return Ok(());
                    }
                },
                Err(_) => {
                    // No active receivers, exit
                    return Ok(());
                }
            }
        }
    }
    
    fn advance_to_next_track(playlist: &crate::models::playlist::Playlist, current_index: usize) {
        let mut new_playlist = playlist.clone();
        new_playlist.current_track = (current_index + 1) % new_playlist.tracks.len();
        crate::services::playlist::save_playlist(&new_playlist, &crate::config::PLAYLIST_FILE);
        crate::services::playlist::invalidate_playlist_cache();
    }
    
    // Public methods remain the same...
    pub fn subscribe(&self) -> (String, broadcast::Receiver<AudioChunk>) {
        let connection_id = uuid::Uuid::new_v4().to_string();
        let receiver = self.broadcast_tx.subscribe();
        
        self.connections.write().insert(connection_id.clone(), ConnectionInfo {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            platform: "unknown".to_string(),
        });
        
        (connection_id, receiver)
    }
    
    pub fn decrement_listener_count(&self, connection_id: &str) {
        self.connections.write().remove(connection_id);
    }
    
    pub fn update_connection_info(&self, connection_id: &str, platform: String, _user_agent: String) {
        if let Some(conn) = self.connections.write().get_mut(connection_id) {
            conn.platform = platform;
        }
    }
    
    pub fn cleanup_stale_connections(&self) {
        let now = Instant::now();
        self.connections.write().retain(|_, conn| {
            now.duration_since(conn.last_heartbeat).as_secs() < 300
        });
    }
    
    pub fn update_listener_heartbeat(&self, connection_id: &str) {
        if let Some(conn) = self.connections.write().get_mut(connection_id) {
            conn.last_heartbeat = Instant::now();
        }
    }
    
    pub fn get_active_listeners(&self) -> usize {
        self.connections.read().len()
    }
    
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::Relaxed)
    }
    
    pub fn stop_broadcasting(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
    }
    
    pub fn get_precise_position(&self) -> (u64, u64) {
        let ms = self.track_position_ms.load(Ordering::Relaxed);
        (ms / 1000, ms % 1000)
    }
    
    pub fn get_track_info(&self) -> Option<String> {
        self.current_track_json.read().clone()
    }
    
    pub fn get_current_bitrate(&self) -> u64 {
        self.track_bitrate.load(Ordering::Relaxed)
    }
    
    pub fn get_current_track_duration(&self) -> u64 {
        self.track_duration_secs.load(Ordering::Relaxed)
    }
    
    pub fn track_ended(&self) -> bool {
        let (pos, _) = self.get_precise_position();
        let duration = self.get_current_track_duration();
        duration > 0 && pos >= duration
    }
    
    pub fn get_track_state(&self) -> TrackState {
        let (pos_secs, pos_ms) = self.get_precise_position();
        let duration = self.get_current_track_duration();
        let remaining = if duration > pos_secs { duration - pos_secs } else { 0 };
        
        TrackState {
            position_seconds: pos_secs,
            position_milliseconds: pos_ms,
            duration,
            remaining_time: remaining,
            is_near_end: remaining <= 10,
            bitrate: self.get_current_bitrate(),
            track_info: self.get_track_info(),
        }
    }
    
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<AudioChunk> {
        self.broadcast_tx.subscribe()
    }
    
    pub fn increment_listener_count(&self) -> String {
        let (id, _) = self.subscribe();
        id
    }
    
    pub fn get_playback_position(&self) -> u64 {
        self.get_precise_position().0
    }
    
    pub fn get_recent_chunks(&self, _from_chunk_id: u64) -> Vec<AudioChunk> {
        Vec::new()
    }
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        self.stop_broadcasting();
        // No need to sleep with async
    }
}
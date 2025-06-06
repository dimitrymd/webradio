// src/services/streamer.rs - Fully optimized with all CPU improvements

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use log::{info, error};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::path::PathBuf;
use dashmap::DashMap;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;
use tokio::time::sleep;
use bytes::Bytes;
use serde_json::Value;

// MP3 frame-aligned streaming with optimized chunk size
const MP3_FRAME_SIZE: usize = 1152;  // Samples per MP3 frame
const FRAMES_PER_CHUNK: usize = 40;   // 40 frames = ~1 second at 44.1kHz
const BROADCAST_CHUNK_SIZE: usize = 4608; // Average MP3 frame size * frames
const BROADCAST_BUFFER_SIZE: usize = 256;

// Pre-allocated chunk with reference counting
#[derive(Clone)]
pub struct AudioChunk {
    pub data: Arc<[u8]>,  // Reference counted, no copying
    pub position: u64,
    pub timestamp: Instant,
    pub chunk_id: u64,
}

// Convert to Bytes for compatibility
impl AudioChunk {
    pub fn to_bytes(&self) -> Bytes {
        Bytes::copy_from_slice(&self.data)
    }
}

// For direct_stream.rs compatibility
impl From<AudioChunk> for Bytes {
    fn from(chunk: AudioChunk) -> Self {
        Bytes::copy_from_slice(&chunk.data)
    }
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

// Cached track data with pre-allocated chunks
#[derive(Clone)]
struct CachedTrack {
    data: Arc<Vec<u8>>,
    audio_start_offset: usize,
    bitrate: u64,
    duration: u64,
    frame_positions: Vec<usize>,
    pre_allocated_chunks: Vec<Arc<[u8]>>, // Pre-allocated chunks
}

// Cache for serialized JSON
struct TrackInfoCache {
    track_json: String,
    track_with_metadata: String,
    last_update: Instant,
}

pub struct StreamManager {
    is_streaming: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    connections: Arc<DashMap<String, ConnectionInfo>>, // Lock-free concurrent map
    
    track_position_ms: Arc<AtomicU64>,
    track_duration_secs: Arc<AtomicU64>,
    track_bitrate: Arc<AtomicU64>,
    
    current_track_json: Arc<RwLock<Option<String>>>,
    track_info_cache: Arc<RwLock<Option<TrackInfoCache>>>, // JSON cache
    track_start_time: Arc<RwLock<Instant>>,
    
    music_folder: PathBuf,
    
    // Track cache
    cached_track: Arc<RwLock<Option<CachedTrack>>>,
    
    // Async runtime handle
    runtime_handle: tokio::runtime::Handle,
}

// Conditional logging macros
#[cfg(debug_assertions)]
macro_rules! debug_log {
    ($($arg:tt)*) => { log::debug!($($arg)*) };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing Fully Optimized StreamManager");
        
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE);
        
        Self {
            is_streaming: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            
            broadcast_tx: Arc::new(broadcast_tx),
            connections: Arc::new(DashMap::new()),
            
            track_position_ms: Arc::new(AtomicU64::new(0)),
            track_duration_secs: Arc::new(AtomicU64::new(0)),
            track_bitrate: Arc::new(AtomicU64::new(192000)),
            
            current_track_json: Arc::new(RwLock::new(None)),
            track_info_cache: Arc::new(RwLock::new(None)),
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
        let track_info_cache = self.track_info_cache.clone();
        let track_start_time = self.track_start_time.clone();
        let track_position_ms = self.track_position_ms.clone();
        let track_duration_secs = self.track_duration_secs.clone();
        let track_bitrate = self.track_bitrate.clone();
        let cached_track = self.cached_track.clone();
        
        // Spawn async task
        self.runtime_handle.spawn(async move {
            Self::async_broadcast_loop(
                music_folder,
                broadcast_tx,
                is_streaming,
                should_stop,
                current_track_json,
                track_info_cache,
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
        track_info_cache: Arc<RwLock<Option<TrackInfoCache>>>,
        track_start_time: Arc<RwLock<Instant>>,
        track_position_ms: Arc<AtomicU64>,
        track_duration_secs: Arc<AtomicU64>,
        track_bitrate: Arc<AtomicU64>,
        cached_track: Arc<RwLock<Option<CachedTrack>>>,
    ) {
        info!("Optimized broadcast loop started");
        is_streaming.store(true, Ordering::Relaxed);
        
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
                    info!("No listeners - clearing track cache");
                    *cached_track.write() = None;
                    *track_info_cache.write() = None;
                }
                
                sleep(Duration::from_secs(30)).await;
                continue;
            }
            
            // Get playlist from cache
            let playlist = crate::services::playlist::get_playlist_cached();
            
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
                sleep(Duration::from_secs(1)).await;
                continue;
            }
            
            // Load track into memory with pre-allocated chunks
            let cached = match Self::load_track_to_memory_optimized(&track_path).await {
                Ok(cached) => {
                    let file_size_mb = cached.data.len() as f64 / (1024.0 * 1024.0);
                    info!("Loaded track: \"{}\" - {:.1}MB, {} chunks", 
                        track.title, file_size_mb, cached.pre_allocated_chunks.len());
                    
                    // Store in cache
                    *cached_track.write() = Some(cached.clone());
                    cached
                },
                Err(e) => {
                    error!("Failed to load track: {}", e);
                    Self::advance_to_next_track(&playlist, track_index);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            
            // Cache track JSON
            Self::cache_track_info(&track_info_cache, &current_track_json, track);
            
            // Update track metadata
            let actual_duration = if track.duration > 0 { track.duration } else { cached.duration };
            let actual_bitrate = cached.bitrate;
            
            {
                *track_start_time.write() = Instant::now();
            }
            
            track_duration_secs.store(actual_duration, Ordering::Relaxed);
            track_bitrate.store(actual_bitrate, Ordering::Relaxed);
            track_position_ms.store(0, Ordering::Relaxed);
            
            info!("Playing: \"{}\" - {}s", track.title, actual_duration);
            
            // Stream using pre-allocated chunks
            let stream_result = Self::stream_with_pre_allocated_chunks(
                &cached,
                &broadcast_tx,
                actual_duration,
                &track_position_ms,
                &should_stop,
            ).await;
            
            match stream_result {
                Ok(_) => debug_log!("Track completed"),
                Err(e) => error!("Streaming error: {}", e),
            }
            
            // Clear cache after track ends
            *cached_track.write() = None;
            *track_info_cache.write() = None;
            
            // Advance to next track
            Self::advance_to_next_track(&playlist, track_index);
            
            // Gap between tracks
            sleep(Duration::from_millis(500)).await;
            
            // Clear track info
            *current_track_json.write() = None;
            track_position_ms.store(0, Ordering::SeqCst);
        }
        
        is_streaming.store(false, Ordering::Relaxed);
    }
    
    async fn load_track_to_memory_optimized(track_path: &PathBuf) -> Result<CachedTrack, Box<dyn std::error::Error + Send + Sync>> {
        // Read entire file into memory
        let mut file = File::open(track_path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len() as usize;
        
        // Use uninitialized memory for faster loading
        let mut data = vec![0u8; file_size];
        file.read_exact(&mut data).await?;
        
        // Find all MP3 frames
        let (audio_start_offset, frame_positions) = Self::analyze_mp3_frames(&data);
        
        // Calculate bitrate
        let bitrate = if frame_positions.len() > 10 {
            let mut total_frame_size = 0;
            for i in 0..frame_positions.len().saturating_sub(1).min(100) {
                total_frame_size += frame_positions[i + 1] - frame_positions[i];
            }
            let avg_frame_size = total_frame_size / frame_positions.len().min(100).max(1);
            let ms_per_frame = 26.12; // 26,122449
            let bytes_per_ms = avg_frame_size as f64 / ms_per_frame;
            (bytes_per_ms * 8.0 * 1000.0) as u64
        } else {
            192000
        };
        
        // Calculate duration
        let duration = (frame_positions.len() as f64 * 26.12 / 1000.0) as u64;
        
        // Pre-allocate all chunks
        let data_arc = Arc::new(data);
        let pre_allocated_chunks = Self::pre_allocate_chunks(&data_arc, &frame_positions);
        
        Ok(CachedTrack {
            data: data_arc,
            audio_start_offset,
            bitrate,
            duration,
            frame_positions,
            pre_allocated_chunks,
        })
    }
    
    fn pre_allocate_chunks(data: &Arc<Vec<u8>>, frame_positions: &[usize]) -> Vec<Arc<[u8]>> {
        let mut chunks = Vec::new();
        
        for chunk_start in (0..frame_positions.len()).step_by(FRAMES_PER_CHUNK) {
            let chunk_end = (chunk_start + FRAMES_PER_CHUNK).min(frame_positions.len());
            
            if chunk_start < frame_positions.len() {
                let start_pos = frame_positions[chunk_start];
                let end_pos = if chunk_end < frame_positions.len() {
                    frame_positions[chunk_end]
                } else {
                    data.len()
                };
                
                // Create Arc slice without copying
                let chunk_data = data[start_pos..end_pos].to_vec();
                chunks.push(Arc::from(chunk_data.into_boxed_slice()));
            }
        }
        
        chunks
    }
    
    fn cache_track_info(
        cache: &Arc<RwLock<Option<TrackInfoCache>>>,
        current_json: &Arc<RwLock<Option<String>>>,
        track: &crate::models::playlist::Track
    ) {
        let track_json = serde_json::to_string(track).unwrap_or_default();
        
        let mut with_metadata = serde_json::from_str::<Value>(&track_json).unwrap_or_default();
        if let Value::Object(ref mut map) = with_metadata {
            // Pre-add all metadata fields
            map.insert("active_listeners".to_string(), Value::Number(0.into()));
            map.insert("bitrate".to_string(), Value::Number(0.into()));
            map.insert("radio_position".to_string(), Value::Number(0.into()));
            map.insert("radio_position_ms".to_string(), Value::Number(0.into()));
            map.insert("streaming_mode".to_string(), Value::String("true-radio".to_string()));
        }
        
        let track_with_metadata = serde_json::to_string(&with_metadata).unwrap_or_default();
        
        *cache.write() = Some(TrackInfoCache {
            track_json: track_json.clone(),
            track_with_metadata,
            last_update: Instant::now(),
        });
        
        *current_json.write() = Some(track_json);
    }
    
    async fn stream_with_pre_allocated_chunks(
        cached: &CachedTrack,
        broadcast_tx: &Arc<broadcast::Sender<AudioChunk>>,
        duration: u64,
        track_position_ms: &Arc<AtomicU64>,
        should_stop: &Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if cached.pre_allocated_chunks.is_empty() {
            return Err("No chunks available".into());
        }
        
        let track_start = Instant::now();
        let ms_per_frame = 26.12;
        let chunk_duration = Duration::from_millis((FRAMES_PER_CHUNK as f64 * ms_per_frame) as u64);
        
        let mut chunk_index = 0;
        let mut last_position_update = Instant::now();
        
        // Send initial burst
        for i in 0..3.min(cached.pre_allocated_chunks.len()) {
            let chunk = AudioChunk {
                data: cached.pre_allocated_chunks[i].clone(),
                position: (i * BROADCAST_CHUNK_SIZE) as u64,
                timestamp: track_start,
                chunk_id: i as u64,
            };
            
            let _ = broadcast_tx.send(chunk);
            chunk_index = i + 1;
        }
        
        // Main streaming loop
        let mut next_send_time = Instant::now() + chunk_duration;
        
        while chunk_index < cached.pre_allocated_chunks.len() {
            // Check exit conditions
            if should_stop.load(Ordering::Relaxed) || broadcast_tx.receiver_count() == 0 {
                return Ok(());
            }
            
            let elapsed = track_start.elapsed();
            if elapsed >= Duration::from_secs(duration) {
                return Ok(());
            }
            
            // Wait for next send time
            let now = Instant::now();
            if next_send_time > now {
                sleep(next_send_time - now).await;
            }
            next_send_time = now + chunk_duration;
            
            // Update position atomically only once per second
            if last_position_update.elapsed() >= Duration::from_secs(1) {
                track_position_ms.store(elapsed.as_millis() as u64, Ordering::Relaxed);
                last_position_update = now;
            }
            
            // Send pre-allocated chunk
            let chunk = AudioChunk {
                data: cached.pre_allocated_chunks[chunk_index].clone(),
                position: (chunk_index * BROADCAST_CHUNK_SIZE) as u64,
                timestamp: now,
                chunk_id: chunk_index as u64,
            };
            
            match broadcast_tx.send(chunk) {
                Ok(_) => chunk_index += 1,
                Err(_) => return Ok(()),
            }
        }
        
        Ok(())
    }
    
    fn analyze_mp3_frames(data: &[u8]) -> (usize, Vec<usize>) {
        let mut frame_positions = Vec::new();
        let mut audio_start = 0;
        
        // Skip ID3v2 tag if present
        let mut pos = if data.len() > 10 && &data[0..3] == b"ID3" {
            let size = ((data[6] as usize & 0x7F) << 21)
                | ((data[7] as usize & 0x7F) << 14)
                | ((data[8] as usize & 0x7F) << 7)
                | (data[9] as usize & 0x7F);
            audio_start = 10 + size;
            10 + size
        } else {
            0
        };
        
        // Find all MP3 frames
        while pos < data.len().saturating_sub(4) {
            if data[pos] == 0xFF && (data[pos + 1] & 0xE0) == 0xE0 {
                let header = ((data[pos] as u32) << 24)
                    | ((data[pos + 1] as u32) << 16)
                    | ((data[pos + 2] as u32) << 8)
                    | (data[pos + 3] as u32);
                
                if let Some(frame_size) = Self::calculate_frame_size(header) {
                    frame_positions.push(pos);
                    pos += frame_size;
                    continue;
                }
            }
            pos += 1;
        }
        
        (audio_start, frame_positions)
    }
    
    fn calculate_frame_size(header: u32) -> Option<usize> {
        let version = (header >> 19) & 3;
        let layer = (header >> 17) & 3;
        let bitrate_index = (header >> 12) & 0xF;
        let sample_rate_index = (header >> 10) & 3;
        let padding = (header >> 9) & 1;
        
        if version == 1 || layer != 1 || bitrate_index == 0 || bitrate_index == 15 || sample_rate_index == 3 {
            return None;
        }
        
        let bitrates = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
        let bitrate = bitrates[bitrate_index as usize] * 1000;
        
        let sample_rates = [44100, 48000, 32000, 0];
        let sample_rate = sample_rates[sample_rate_index as usize];
        
        if bitrate == 0 || sample_rate == 0 {
            return None;
        }
        
        let frame_size = (144 * bitrate / sample_rate + padding) as usize;
        Some(frame_size)
    }
    
    fn advance_to_next_track(playlist: &crate::models::playlist::Playlist, current_index: usize) {
        let mut new_playlist = playlist.clone();
        new_playlist.current_track = (current_index + 1) % new_playlist.tracks.len();
        crate::services::playlist::save_playlist(&new_playlist, &crate::config::PLAYLIST_FILE);
    }
    
    // Optimized public methods using DashMap
    pub fn subscribe(&self) -> (String, broadcast::Receiver<AudioChunk>) {
        let connection_id = uuid::Uuid::new_v4().to_string();
        let receiver = self.broadcast_tx.subscribe();
        
        self.connections.insert(connection_id.clone(), ConnectionInfo {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            platform: "unknown".to_string(),
        });
        
        (connection_id, receiver)
    }
    
    pub fn decrement_listener_count(&self, connection_id: &str) {
        self.connections.remove(connection_id);
    }
    
    pub fn update_connection_info(&self, connection_id: &str, platform: String, _user_agent: String) {
        if let Some(mut conn) = self.connections.get_mut(connection_id) {
            conn.platform = platform;
        }
    }
    
    pub fn cleanup_stale_connections(&self) {
        let now = Instant::now();
        self.connections.retain(|_, conn| {
            now.duration_since(conn.last_heartbeat).as_secs() < 300
        });
    }
    
    pub fn update_listener_heartbeat(&self, connection_id: &str) {
        if let Some(mut conn) = self.connections.get_mut(connection_id) {
            conn.last_heartbeat = Instant::now();
        }
    }
    
    pub fn get_active_listeners(&self) -> usize {
        self.connections.len()
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
    
    pub fn get_cached_track_info(&self) -> Option<String> {
        self.track_info_cache.read().as_ref().map(|c| c.track_with_metadata.clone())
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
    }
}
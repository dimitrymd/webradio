// src/services/streamer.rs - Fixed track switching and logging

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::{Mutex, RwLock};
use log::{info, warn, debug, error};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::path::PathBuf;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use tokio::sync::broadcast;
use bytes::Bytes;

const BROADCAST_CHUNK_SIZE: usize = 8192;
const BROADCAST_BUFFER_SIZE: usize = 100;
const HEARTBEAT_TIMEOUT: u64 = 60;

// Track end reasons - moved outside impl block
#[derive(Debug)]
enum TrackEndReason {
    Finished,
    Interrupted,
    Error,
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
    last_chunk_id: u64,
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

// Current track information
#[derive(Clone, Debug)]
pub struct CurrentTrackInfo {
    pub track: crate::models::playlist::Track,
    pub start_time: Instant,
    pub file_size: u64,
    pub bitrate: u64,
}

#[derive(Clone)]
pub struct StreamManager {
    // Control flags
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    should_switch_track: Arc<AtomicBool>,
    active_listeners: Arc<AtomicUsize>,
    current_chunk_id: Arc<AtomicUsize>,
    
    // Broadcast channel
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    
    // Connection tracking
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    
    // Current track state (synchronized)
    current_track: Arc<RwLock<Option<CurrentTrackInfo>>>,
    
    // Music folder
    music_folder: PathBuf,
    
    // Recent chunks for late joiners
    recent_chunks: Arc<Mutex<VecDeque<AudioChunk>>>,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager with proper track switching");
        
        // Create broadcast channel with larger capacity
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE * 2);
        
        Self {
            is_streaming: Arc::new(AtomicBool::new(false)),
            track_ended: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            should_switch_track: Arc::new(AtomicBool::new(false)),
            active_listeners: Arc::new(AtomicUsize::new(0)),
            current_chunk_id: Arc::new(AtomicUsize::new(0)),
            
            broadcast_tx: Arc::new(broadcast_tx),
            connections: Arc::new(RwLock::new(HashMap::new())),
            current_track: Arc::new(RwLock::new(None)),
            
            music_folder: music_folder.to_path_buf(),
            recent_chunks: Arc::new(Mutex::new(VecDeque::with_capacity(BROADCAST_BUFFER_SIZE))),
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        if self.is_streaming.load(Ordering::SeqCst) {
            warn!("Broadcast already running");
            return;
        }
        
        let music_folder = self.music_folder.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        let should_stop = self.should_stop.clone();
        let should_switch_track = self.should_switch_track.clone();
        let current_track = self.current_track.clone();
        let current_chunk_id = self.current_chunk_id.clone();
        let recent_chunks = self.recent_chunks.clone();
        
        info!("Starting broadcast thread with proper track management");
        
        thread::spawn(move || {
            Self::radio_broadcast_loop(
                music_folder,
                broadcast_tx,
                is_streaming,
                track_ended,
                should_stop,
                should_switch_track,
                current_track,
                current_chunk_id,
                recent_chunks,
            );
        });
    }
    
    fn radio_broadcast_loop(
        music_folder: PathBuf,
        broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        should_switch_track: Arc<AtomicBool>,
        current_track: Arc<RwLock<Option<CurrentTrackInfo>>>,
        current_chunk_id: Arc<AtomicUsize>,
        recent_chunks: Arc<Mutex<VecDeque<AudioChunk>>>,
    ) {
        info!("Radio broadcast loop started");
        is_streaming.store(true, Ordering::SeqCst);
        
        let mut current_track_index = 0usize;
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get playlist
            let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                info!("No tracks available, broadcasting silence");
                Self::broadcast_silence(&broadcast_tx, &should_stop, &current_chunk_id, &recent_chunks);
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            
            // Check if we should switch tracks
            if should_switch_track.load(Ordering::SeqCst) {
                should_switch_track.store(false, Ordering::SeqCst);
                current_track_index = (current_track_index + 1) % playlist.tracks.len();
                info!("Manual track switch requested, moving to track {}", current_track_index);
            }
            
            // Get current track
            if current_track_index >= playlist.tracks.len() {
                current_track_index = 0;
            }
            
            let track = playlist.tracks[current_track_index].clone();
            let track_path = music_folder.join(&track.path);
            
            if !track_path.exists() {
                error!("Track not found: {} - skipping", track_path.display());
                current_track_index = (current_track_index + 1) % playlist.tracks.len();
                continue;
            }
            
            // Get file metadata
            let file_size = match std::fs::metadata(&track_path) {
                Ok(metadata) => metadata.len(),
                Err(e) => {
                    error!("Cannot read file metadata for {}: {}", track_path.display(), e);
                    current_track_index = (current_track_index + 1) % playlist.tracks.len();
                    continue;
                }
            };
            
            // Calculate bitrate
            let bitrate = if track.duration > 0 {
                (file_size * 8) / track.duration
            } else {
                128000 // Default
            };
            
            // Update current track info FIRST
            let track_info = CurrentTrackInfo {
                track: track.clone(),
                start_time: Instant::now(),
                file_size,
                bitrate,
            };
            
            {
                let mut current_track_lock = current_track.write();
                *current_track_lock = Some(track_info.clone());
            }
            
            info!("NOW BROADCASTING: \"{}\" by \"{}\" ({}s, {}kb, ~{}kbps)", 
                  track.title, track.artist, track.duration, file_size / 1024, bitrate / 1000);
            
            // Open and broadcast the track
            match File::open(&track_path) {
                Ok(mut file) => {
                    track_ended.store(false, Ordering::SeqCst);
                    
                    // Skip ID3 tags
                    let id3_size = Self::detect_id3_size(&mut file).unwrap_or(0);
                    if id3_size > 0 {
                        let _ = file.seek(SeekFrom::Start(id3_size));
                        info!("Skipped {} bytes of ID3 tags", id3_size);
                    }
                    
                    // Broadcast the track
                    let broadcast_result = Self::broadcast_track(
                        &broadcast_tx,
                        &should_stop,
                        &should_switch_track,
                        &current_chunk_id,
                        &recent_chunks,
                        file,
                        &track_info,
                    );
                    
                    match broadcast_result {
                        TrackEndReason::Finished => {
                            info!("Track \"{}\" finished naturally", track.title);
                            current_track_index = (current_track_index + 1) % playlist.tracks.len();
                        },
                        TrackEndReason::Interrupted => {
                            info!("Track \"{}\" interrupted for switch", track.title);
                            // Index already updated by switch logic
                        },
                        TrackEndReason::Error => {
                            error!("Track \"{}\" ended with error", track.title);
                            current_track_index = (current_track_index + 1) % playlist.tracks.len();
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to open track {}: {}", track_path.display(), e);
                    current_track_index = (current_track_index + 1) % playlist.tracks.len();
                }
            }
            
            // Mark track as ended
            track_ended.store(true, Ordering::SeqCst);
            
            // Update playlist file with new current track
            let mut updated_playlist = playlist.clone();
            updated_playlist.current_track = current_track_index;
            crate::services::playlist::save_playlist(&updated_playlist, &crate::config::PLAYLIST_FILE);
            
            // Brief pause between tracks
            thread::sleep(Duration::from_millis(500));
        }
        
        info!("Radio broadcast loop ending");
        is_streaming.store(false, Ordering::SeqCst);
    }
    
    fn broadcast_track(
        broadcast_tx: &Arc<broadcast::Sender<AudioChunk>>,
        should_stop: &Arc<AtomicBool>,
        should_switch_track: &Arc<AtomicBool>,
        current_chunk_id: &Arc<AtomicUsize>,
        recent_chunks: &Arc<Mutex<VecDeque<AudioChunk>>>,
        mut file: File,
        track_info: &CurrentTrackInfo,
    ) -> TrackEndReason {
        let mut chunk_buffer = vec![0u8; BROADCAST_CHUNK_SIZE];
        let mut total_bytes_read = 0u64;
        let start_time = Instant::now();
        
        // Calculate timing for real-time playback
        let bytes_per_second = if track_info.bitrate > 0 {
            track_info.bitrate / 8
        } else {
            16000 // Default 128kbps = 16KB/s
        };
        
        let expected_duration = Duration::from_secs(track_info.track.duration);
        
        info!("Broadcasting track: {}kbps, expected duration: {}s", 
              track_info.bitrate / 1000, track_info.track.duration);
        
        loop {
            // Check stop conditions
            if should_stop.load(Ordering::SeqCst) {
                return TrackEndReason::Interrupted;
            }
            
            if should_switch_track.load(Ordering::SeqCst) {
                info!("Track switch requested during playback");
                return TrackEndReason::Interrupted;
            }
            
            // Check if we should end based on duration
            let elapsed = start_time.elapsed();
            if elapsed >= expected_duration {
                info!("Track duration reached ({:?}), ending naturally", elapsed);
                return TrackEndReason::Finished;
            }
            
            // Read chunk from file
            let bytes_read = match file.read(&mut chunk_buffer) {
                Ok(0) => {
                    info!("End of file reached after {:?}", elapsed);
                    return TrackEndReason::Finished;
                },
                Ok(n) => n,
                Err(e) => {
                    error!("Read error: {}", e);
                    return TrackEndReason::Error;
                }
            };
            
            total_bytes_read += bytes_read as u64;
            
            // Create audio chunk
            let chunk_id = current_chunk_id.fetch_add(1, Ordering::SeqCst) as u64;
            let chunk = AudioChunk {
                data: Bytes::copy_from_slice(&chunk_buffer[..bytes_read]),
                position: total_bytes_read,
                timestamp: Instant::now(),
                chunk_id,
            };
            
            // Update recent chunks buffer
            {
                let mut chunks = recent_chunks.lock();
                if chunks.len() >= BROADCAST_BUFFER_SIZE {
                    chunks.pop_front();
                }
                chunks.push_back(chunk.clone());
            }
            
            // Broadcast to all listeners
            match broadcast_tx.send(chunk) {
                Ok(receiver_count) => {
                    if receiver_count > 0 && chunk_id % 100 == 0 {
                        debug!("Broadcast chunk {} to {} listeners", chunk_id, receiver_count);
                    }
                },
                Err(_) => {
                    // Channel full or no receivers - this is OK
                }
            }
            
            // Real-time timing control
            let expected_time = Duration::from_millis((total_bytes_read * 1000) / bytes_per_second);
            let actual_time = start_time.elapsed();
            
            if expected_time > actual_time {
                let sleep_time = expected_time - actual_time;
                if sleep_time > Duration::from_millis(1) && sleep_time < Duration::from_millis(1000) {
                    thread::sleep(sleep_time);
                }
            }
        }
    }
    
    fn broadcast_silence(
        broadcast_tx: &Arc<broadcast::Sender<AudioChunk>>,
        should_stop: &Arc<AtomicBool>,
        current_chunk_id: &Arc<AtomicUsize>,
        recent_chunks: &Arc<Mutex<VecDeque<AudioChunk>>>,
    ) {
        let silence_data = vec![0u8; BROADCAST_CHUNK_SIZE];
        
        for i in 0..20 {
            if should_stop.load(Ordering::SeqCst) {
                break;
            }
            
            let chunk_id = current_chunk_id.fetch_add(1, Ordering::SeqCst) as u64;
            let chunk = AudioChunk {
                data: Bytes::from(silence_data.clone()),
                position: i * BROADCAST_CHUNK_SIZE as u64,
                timestamp: Instant::now(),
                chunk_id,
            };
            
            {
                let mut chunks = recent_chunks.lock();
                if chunks.len() >= BROADCAST_BUFFER_SIZE {
                    chunks.pop_front();
                }
                chunks.push_back(chunk.clone());
            }
            
            let _ = broadcast_tx.send(chunk);
            thread::sleep(Duration::from_millis(250));
        }
    }
    
    fn detect_id3_size(file: &mut File) -> Option<u64> {
        let mut header = [0u8; 10];
        
        if file.read_exact(&mut header).is_err() {
            return Some(0);
        }
        
        if &header[0..3] == b"ID3" {
            let size = ((header[6] as u32 & 0x7F) << 21) |
                      ((header[7] as u32 & 0x7F) << 14) |
                      ((header[8] as u32 & 0x7F) << 7) |
                      (header[9] as u32 & 0x7F);
            
            let total_size = (size + 10) as u64;
            Some(total_size)
        } else {
            Some(0)
        }
    }
    
    // Public method to request track switch
    pub fn request_track_switch(&self) {
        info!("Track switch requested");
        self.should_switch_track.store(true, Ordering::SeqCst);
    }
    
    // Connection management
    pub fn subscribe(&self) -> (String, broadcast::Receiver<AudioChunk>) {
        let connection_id = uuid::Uuid::new_v4().to_string();
        let receiver = self.broadcast_tx.subscribe();
        
        {
            let mut connections = self.connections.write();
            connections.insert(connection_id.clone(), ConnectionInfo {
                connected_at: Instant::now(),
                last_heartbeat: Instant::now(),
                platform: "unknown".to_string(),
                last_chunk_id: 0,
            });
        }
        
        self.update_listener_count();
        info!("New listener subscribed: {}", &connection_id[..8]);
        
        (connection_id, receiver)
    }
    
    pub fn get_recent_chunks(&self, from_chunk_id: u64) -> Vec<AudioChunk> {
        let chunks = self.recent_chunks.lock();
        chunks
            .iter()
            .filter(|chunk| chunk.chunk_id > from_chunk_id)
            .cloned()
            .collect()
    }
    
    pub fn increment_listener_count(&self) -> String {
        let (connection_id, _) = self.subscribe();
        connection_id
    }
    
    pub fn decrement_listener_count(&self, connection_id: &str) {
        {
            let mut connections = self.connections.write();
            connections.remove(connection_id);
        }
        self.update_listener_count();
    }
    
    pub fn update_connection_info(&self, connection_id: &str, platform: String, _user_agent: String) {
        let mut connections = self.connections.write();
        if let Some(conn_info) = connections.get_mut(connection_id) {
            conn_info.platform = platform;
        }
    }
    
    fn update_listener_count(&self) {
        let count = self.connections.read().len();
        self.active_listeners.store(count, Ordering::SeqCst);
    }
    
    pub fn cleanup_stale_connections(&self) {
        let now = Instant::now();
        let mut connections = self.connections.write();
        
        let before_count = connections.len();
        connections.retain(|_id, conn_info| {
            now.duration_since(conn_info.last_heartbeat).as_secs() < HEARTBEAT_TIMEOUT
        });
        let after_count = connections.len();
        
        if before_count != after_count {
            info!("Cleaned up {} stale connections", before_count - after_count);
            self.active_listeners.store(after_count, Ordering::SeqCst);
        }
    }
    
    pub fn get_active_listeners(&self) -> usize {
        self.active_listeners.load(Ordering::SeqCst)
    }
    
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::SeqCst)
    }
    
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<AudioChunk> {
        self.broadcast_tx.subscribe()
    }
    
    pub fn get_playback_position(&self) -> u64 {
        if let Some(track_info) = self.current_track.read().as_ref() {
            track_info.start_time.elapsed().as_secs()
        } else {
            0
        }
    }
    
    pub fn get_precise_position(&self) -> (u64, u64) {
        if let Some(track_info) = self.current_track.read().as_ref() {
            let elapsed = track_info.start_time.elapsed();
            (elapsed.as_secs(), elapsed.subsec_millis() as u64)
        } else {
            (0, 0)
        }
    }
    
    pub fn get_track_info(&self) -> Option<String> {
        if let Some(track_info) = self.current_track.read().as_ref() {
            serde_json::to_string(&track_info.track).ok()
        } else {
            None
        }
    }
    
    pub fn get_current_bitrate(&self) -> u64 {
        if let Some(track_info) = self.current_track.read().as_ref() {
            track_info.bitrate
        } else {
            128000
        }
    }
    
    pub fn get_current_track_duration(&self) -> u64 {
        if let Some(track_info) = self.current_track.read().as_ref() {
            track_info.track.duration
        } else {
            0
        }
    }
    
    pub fn track_ended(&self) -> bool {
        self.track_ended.load(Ordering::SeqCst)
    }
    
    pub fn get_track_state(&self) -> TrackState {
        let (position_secs, position_ms) = self.get_precise_position();
        let duration = self.get_current_track_duration();
        let remaining = if duration > position_secs { duration - position_secs } else { 0 };
        let is_near_end = remaining <= 10;
        
        TrackState {
            position_seconds: position_secs,
            position_milliseconds: position_ms,
            duration,
            remaining_time: remaining,
            is_near_end,
            bitrate: self.get_current_bitrate(),
            track_info: self.get_track_info(),
        }
    }
    
    pub fn update_listener_heartbeat(&self, connection_id: &str) {
        let mut connections = self.connections.write();
        if let Some(conn_info) = connections.get_mut(connection_id) {
            conn_info.last_heartbeat = Instant::now();
        }
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping radio broadcast");
        self.should_stop.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(100));
    }
}
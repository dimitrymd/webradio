// src/services/streamer.rs - Ultra low CPU implementation

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use log::{info, error};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::path::PathBuf;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, BufReader};
use tokio::sync::broadcast;
use bytes::Bytes;

// Larger chunks = fewer operations
const BROADCAST_CHUNK_SIZE: usize = 65536;  // 64KB chunks
const BROADCAST_BUFFER_SIZE: usize = 50;    
const FILE_BUFFER_SIZE: usize = 131072;     // 128KB file buffer

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
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing Ultra Low CPU StreamManager");
        
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
        
        thread::Builder::new()
            .name("radio-broadcast".to_string())
            .spawn(move || {
                // Set lowest priority
                #[cfg(unix)]
                {
                    unsafe {
                        libc::nice(19);
                    }
                }
                
                Self::ultra_low_cpu_broadcast_loop(
                    music_folder,
                    broadcast_tx,
                    is_streaming,
                    should_stop,
                    current_track_json,
                    track_start_time,
                    track_position_ms,
                    track_duration_secs,
                    track_bitrate,
                );
            })
            .expect("Failed to spawn broadcast thread");
    }
    
    fn ultra_low_cpu_broadcast_loop(
        music_folder: PathBuf,
        broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
        is_streaming: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        current_track_json: Arc<RwLock<Option<String>>>,
        track_start_time: Arc<RwLock<Instant>>,
        track_position_ms: Arc<AtomicU64>,
        track_duration_secs: Arc<AtomicU64>,
        track_bitrate: Arc<AtomicU64>,
    ) {
        info!("Ultra low CPU broadcast loop started");
        is_streaming.store(true, Ordering::Relaxed);
        
        let mut cached_playlist = None;
        let mut last_playlist_check = Instant::now();
        
        'main: loop {
            if should_stop.load(Ordering::Relaxed) {
                break 'main;
            }
            
            // When no listeners, just sleep for a long time
            if broadcast_tx.receiver_count() == 0 {
                thread::sleep(Duration::from_secs(10));
                
                // Update virtual position
                if let start_time = track_start_time.read().clone() {
                    let elapsed = start_time.elapsed().as_millis() as u64;
                    track_position_ms.store(elapsed, Ordering::Relaxed);
                }
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
                thread::sleep(Duration::from_secs(30));
                continue;
            }
            
            let track_index = playlist.current_track % playlist.tracks.len();
            let track = &playlist.tracks[track_index];
            let track_path = music_folder.join(&track.path);
            
            if !track_path.exists() {
                Self::advance_to_next_track(&playlist, track_index);
                cached_playlist = None;
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            
            // Use pre-calculated values
            let actual_duration = track.duration.max(180);
            let actual_bitrate = 192000u64; // Fixed bitrate
            
            // Update track info
            {
                *current_track_json.write() = serde_json::to_string(&track).ok();
                *track_start_time.write() = Instant::now();
            }
            
            track_duration_secs.store(actual_duration, Ordering::Relaxed);
            track_bitrate.store(actual_bitrate, Ordering::Relaxed);
            track_position_ms.store(0, Ordering::Relaxed);
            
            info!("Playing: \"{}\" - {}s", track.title, actual_duration);
            
            // Virtual playback when no listeners
            let track_start = Instant::now();
            
            'track: loop {
                if should_stop.load(Ordering::Relaxed) {
                    break 'main;
                }
                
                let elapsed = track_start.elapsed();
                if elapsed >= Duration::from_secs(actual_duration) {
                    break 'track;
                }
                
                // Check for listeners
                if broadcast_tx.receiver_count() == 0 {
                    // No listeners - just update position and sleep
                    track_position_ms.store(elapsed.as_millis() as u64, Ordering::Relaxed);
                    thread::sleep(Duration::from_secs(5));
                    continue 'track;
                }
                
                // We have listeners - need to actually stream
                // Open file only when needed
                let file = match File::open(&track_path) {
                    Ok(f) => f,
                    Err(e) => {
                        error!("Cannot open file: {}", e);
                        break 'track;
                    }
                };
                
                let mut reader = BufReader::with_capacity(FILE_BUFFER_SIZE, file);
                let _ = Self::skip_id3_simple(&mut reader);
                
                // Calculate where we should be in the file
                let elapsed_ms = elapsed.as_millis() as u64;
                let bytes_per_ms = actual_bitrate / 8000;
                let target_position = elapsed_ms * bytes_per_ms;
                
                // Seek to approximate position
                if target_position > 0 {
                    let _ = reader.seek(SeekFrom::Start(target_position));
                }
                
                // Pre-calculate timing
                let chunk_duration_ms = (BROADCAST_CHUNK_SIZE as u64 * 8000) / actual_bitrate;
                let mut buffer = vec![0u8; BROADCAST_CHUNK_SIZE];
                let mut chunk_id = target_position / BROADCAST_CHUNK_SIZE as u64;
                let mut total_bytes = target_position;
                let mut next_chunk_time = Instant::now();
                
                // Stream while we have listeners
                while broadcast_tx.receiver_count() > 0 {
                    if should_stop.load(Ordering::Relaxed) {
                        break 'main;
                    }
                    
                    // Check if track should end
                    if track_start.elapsed() >= Duration::from_secs(actual_duration) {
                        break 'track;
                    }
                    
                    // Update position occasionally
                    track_position_ms.store(track_start.elapsed().as_millis() as u64, Ordering::Relaxed);
                    
                    // Read chunk
                    match reader.read_exact(&mut buffer) {
                        Ok(()) => {
                            total_bytes += BROADCAST_CHUNK_SIZE as u64;
                            chunk_id += 1;
                            
                            let chunk = AudioChunk {
                                data: Bytes::copy_from_slice(&buffer),
                                position: total_bytes,
                                timestamp: Instant::now(),
                                chunk_id,
                            };
                            
                            let _ = broadcast_tx.send(chunk);
                            
                            // Precise timing
                            next_chunk_time += Duration::from_millis(chunk_duration_ms);
                            let now = Instant::now();
                            if next_chunk_time > now {
                                thread::sleep(next_chunk_time - now);
                            } else {
                                next_chunk_time = now;
                            }
                        },
                        Err(_) => {
                            // Try partial read
                            match reader.read(&mut buffer) {
                                Ok(0) => break 'track,
                                Ok(n) => {
                                    if n > 0 {
                                        let chunk = AudioChunk {
                                            data: Bytes::copy_from_slice(&buffer[..n]),
                                            position: total_bytes + n as u64,
                                            timestamp: Instant::now(),
                                            chunk_id: chunk_id + 1,
                                        };
                                        let _ = broadcast_tx.send(chunk);
                                    }
                                    break 'track;
                                },
                                Err(_) => break 'track,
                            }
                        }
                    }
                }
                
                // Listeners disconnected - go back to virtual playback
                drop(reader); // Close file
            }
            
            // Advance to next track
            Self::advance_to_next_track(&playlist, track_index);
            cached_playlist = None;
            
            // Gap between tracks
            thread::sleep(Duration::from_millis(500));
            
            // Clear track info
            *current_track_json.write() = None;
            track_position_ms.store(0, Ordering::SeqCst);
        }
        
        is_streaming.store(false, Ordering::Relaxed);
    }
    
    fn skip_id3_simple(reader: &mut BufReader<File>) -> std::io::Result<()> {
        let mut header = [0u8; 3];
        if reader.read(&mut header)? == 3 && &header == b"ID3" {
            let _ = reader.seek(SeekFrom::Current(1024));
        } else {
            let _ = reader.seek(SeekFrom::Start(0));
        }
        Ok(())
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
        thread::sleep(Duration::from_millis(100));
    }
}
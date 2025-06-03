// src/services/streamer.rs - Adaptive bitrate implementation

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::{Mutex, RwLock};
use log::{info, error, debug};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::path::PathBuf;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, BufReader};
use tokio::sync::broadcast;
use bytes::Bytes;

const BROADCAST_CHUNK_SIZE: usize = 8192; // 8KB chunks
const BROADCAST_BUFFER_SIZE: usize = 100;

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

#[derive(Clone)]
pub struct StreamManager {
    is_streaming: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    
    current_track_json: Arc<RwLock<Option<String>>>,
    track_start_time: Arc<RwLock<Instant>>,
    track_duration: Arc<RwLock<u64>>,
    track_bitrate: Arc<RwLock<u64>>,
    
    music_folder: PathBuf,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing adaptive bitrate StreamManager");
        
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE);
        
        Self {
            is_streaming: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            
            broadcast_tx: Arc::new(broadcast_tx),
            connections: Arc::new(RwLock::new(HashMap::new())),
            
            current_track_json: Arc::new(RwLock::new(None)),
            track_start_time: Arc::new(RwLock::new(Instant::now())),
            track_duration: Arc::new(RwLock::new(0)),
            track_bitrate: Arc::new(RwLock::new(128000)),
            
            music_folder: music_folder.to_path_buf(),
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        if self.is_streaming.load(Ordering::SeqCst) {
            return;
        }
        
        let music_folder = self.music_folder.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let is_streaming = self.is_streaming.clone();
        let should_stop = self.should_stop.clone();
        
        let current_track_json = self.current_track_json.clone();
        let track_start_time = self.track_start_time.clone();
        let track_duration = self.track_duration.clone();
        let track_bitrate = self.track_bitrate.clone();
        
        thread::Builder::new()
            .name("radio-broadcast".to_string())
            .spawn(move || {
                Self::broadcast_loop(
                    music_folder,
                    broadcast_tx,
                    is_streaming,
                    should_stop,
                    current_track_json,
                    track_start_time,
                    track_duration,
                    track_bitrate,
                );
            })
            .expect("Failed to spawn broadcast thread");
        
        info!("Broadcast thread spawned");
    }
    
    fn broadcast_loop(
        music_folder: PathBuf,
        broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
        is_streaming: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        current_track_json: Arc<RwLock<Option<String>>>,
        track_start_time: Arc<RwLock<Instant>>,
        track_duration: Arc<RwLock<u64>>,
        track_bitrate: Arc<RwLock<u64>>,
    ) {
        info!("Adaptive bitrate broadcast loop started");
        is_streaming.store(true, Ordering::SeqCst);
        
        let mut chunk_id = 0u64;
        
        'main: loop {
            if should_stop.load(Ordering::SeqCst) {
                break 'main;
            }
            
            let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
            if playlist.tracks.is_empty() {
                thread::sleep(Duration::from_secs(5));
                continue;
            }
            
            let track_index = playlist.current_track % playlist.tracks.len();
            let track = &playlist.tracks[track_index];
            let track_path = music_folder.join(&track.path);
            
            if !track_path.exists() {
                error!("Track not found: {}", track_path.display());
                Self::advance_to_next_track(&playlist, track_index);
                continue;
            }
            
            // Get file info and detect actual bitrate
            let file_size = match std::fs::metadata(&track_path) {
                Ok(metadata) => metadata.len(),
                Err(e) => {
                    error!("Cannot read file metadata: {}", e);
                    Self::advance_to_next_track(&playlist, track_index);
                    continue;
                }
            };
            
            // Get actual duration
            let actual_duration = match mp3_duration::from_path(&track_path) {
                Ok(d) => d.as_secs(),
                Err(_) => track.duration
            };
            
            // Calculate actual bitrate from file size and duration
            let actual_bitrate = if actual_duration > 0 {
                (file_size * 8) / actual_duration
            } else {
                192000 // Default to 192kbps if we can't determine
            };
            
            // Open file
            let file = match File::open(&track_path) {
                Ok(f) => f,
                Err(e) => {
                    error!("Cannot open file: {}", e);
                    Self::advance_to_next_track(&playlist, track_index);
                    continue;
                }
            };
            
            let mut reader = BufReader::new(file);
            
            // Update track info
            {
                *current_track_json.write() = serde_json::to_string(&track).ok();
                *track_start_time.write() = Instant::now();
                *track_duration.write() = actual_duration;
                *track_bitrate.write() = actual_bitrate;
            }
            
            info!("Playing: \"{}\" by \"{}\" - Duration: {}s, Bitrate: {}kbps", 
                 track.title, track.artist, actual_duration, actual_bitrate / 1000);
            
            // Skip ID3 if present
            let _ = Self::skip_id3(&mut reader);
            
            // Broadcast the track with adaptive timing
            let track_start = Instant::now();
            let mut total_bytes = 0u64;
            let mut buffer = vec![0u8; BROADCAST_CHUNK_SIZE];
            
            // Calculate timing based on actual bitrate
            let bytes_per_second = actual_bitrate / 8;
            let bytes_per_ms = bytes_per_second as f64 / 1000.0;
            
            // Track timing
            let mut last_chunk_time = Instant::now();
            
            loop {
                if should_stop.load(Ordering::SeqCst) {
                    break 'main;
                }
                
                // Check track duration
                if track_start.elapsed() >= Duration::from_secs(actual_duration) {
                    info!("Track duration reached");
                    break;
                }
                
                // Read chunk
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        info!("End of file");
                        break;
                    },
                    Ok(n) => {
                        total_bytes += n as u64;
                        chunk_id += 1;
                        
                        // Send chunk
                        let chunk = AudioChunk {
                            data: Bytes::copy_from_slice(&buffer[..n]),
                            position: total_bytes,
                            timestamp: Instant::now(),
                            chunk_id,
                        };
                        
                        let _ = broadcast_tx.send(chunk);
                        
                        // Calculate how long this chunk should take to play
                        let chunk_duration_ms = n as f64 / bytes_per_ms;
                        let target_next_chunk_time = last_chunk_time + Duration::from_millis(chunk_duration_ms as u64);
                        
                        // Sleep until it's time for the next chunk
                        let now = Instant::now();
                        if target_next_chunk_time > now {
                            thread::sleep(target_next_chunk_time - now);
                        }
                        
                        last_chunk_time = target_next_chunk_time;
                        
                        // Log progress occasionally
                        if chunk_id % 100 == 0 {
                            let elapsed = track_start.elapsed().as_secs();
                            debug!("Progress: {}s / {}s", elapsed, actual_duration);
                        }
                    },
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }
            }
            
            info!("Track finished after {} seconds", track_start.elapsed().as_secs());
            
            // Advance to next track
            Self::advance_to_next_track(&playlist, track_index);
            
            // Small gap between tracks
            thread::sleep(Duration::from_millis(500));
        }
        
        info!("Broadcast loop ending");
        is_streaming.store(false, Ordering::SeqCst);
    }
    
    fn skip_id3(reader: &mut BufReader<File>) -> std::io::Result<()> {
        let mut header = [0u8; 10];
        let bytes_read = reader.read(&mut header)?;
        
        if bytes_read == 10 && &header[0..3] == b"ID3" {
            let size = ((header[6] as u32 & 0x7F) << 21) |
                      ((header[7] as u32 & 0x7F) << 14) |
                      ((header[8] as u32 & 0x7F) << 7) |
                      (header[9] as u32 & 0x7F);
            
            reader.seek(SeekFrom::Current(size as i64))?;
            debug!("Skipped ID3 tag: {} bytes", size + 10);
        } else {
            // Not ID3 or couldn't read full header, seek back
            reader.seek(SeekFrom::Start(0))?;
        }
        
        Ok(())
    }
    
    fn advance_to_next_track(playlist: &crate::models::playlist::Playlist, current_index: usize) {
        let mut new_playlist = playlist.clone();
        new_playlist.current_track = (current_index + 1) % new_playlist.tracks.len();
        crate::services::playlist::save_playlist(&new_playlist, &crate::config::PLAYLIST_FILE);
    }
    
    // Public methods
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
            now.duration_since(conn.last_heartbeat).as_secs() < 60
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
        self.is_streaming.load(Ordering::SeqCst)
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping broadcast");
        self.should_stop.store(true, Ordering::SeqCst);
    }
    
    pub fn get_precise_position(&self) -> (u64, u64) {
        let elapsed = self.track_start_time.read().elapsed();
        (elapsed.as_secs(), elapsed.subsec_millis() as u64)
    }
    
    pub fn get_track_info(&self) -> Option<String> {
        self.current_track_json.read().clone()
    }
    
    pub fn get_current_bitrate(&self) -> u64 {
        *self.track_bitrate.read()
    }
    
    pub fn get_current_track_duration(&self) -> u64 {
        *self.track_duration.read()
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
    
    // Compatibility methods
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
        Vec::new() // Simplified - no chunk caching
    }
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        self.stop_broadcasting();
        thread::sleep(Duration::from_millis(100));
    }
}
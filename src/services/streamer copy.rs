// src/services/streamer.rs - True Radio Broadcast Implementation

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

const BROADCAST_CHUNK_SIZE: usize = 4096; // 4KB chunks for fine-grained streaming
const BROADCAST_BUFFER_SIZE: usize = 50; // Keep 50 chunks in memory (~200KB)
const CHUNKS_PER_SECOND_128KBPS: usize = 4; // 128kbps = 16KB/s = 4 chunks/s

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

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
    
    // Single broadcast channel for all listeners
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    
    // Active connections
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    active_listeners: Arc<AtomicUsize>,
    
    // Control flags
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    
    // Current track info
    current_track_info: Option<String>,
    current_track_path: Option<PathBuf>,
    current_file: Option<File>,
    
    // Playback tracking
    playback_position: u64,
    track_start_time: Instant,
    current_chunk_id: u64,
    
    // Single broadcast thread
    broadcast_thread: Option<thread::JoinHandle<()>>,
    should_stop: Arc<AtomicBool>,
    
    // Track metadata
    current_bitrate: u64,
    current_track_duration: u64,
    
    // Circular buffer for recent chunks (for late joiners)
    recent_chunks: VecDeque<AudioChunk>,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing TRUE RADIO StreamManager - single broadcast for all listeners");
        
        let should_stop = Arc::new(AtomicBool::new(false));
        let now = Instant::now();
        
        // Create broadcast channel with buffer
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE);
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            current_track_info: None,
            current_track_path: None,
            current_file: None,
            playback_position: 0,
            track_start_time: now,
            current_chunk_id: 0,
            broadcast_thread: None,
            should_stop: should_stop.clone(),
            current_bitrate: 128000,
            current_track_duration: 0,
            recent_chunks: VecDeque::with_capacity(BROADCAST_BUFFER_SIZE),
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
            broadcast_tx: Arc::new(broadcast_tx),
            connections: Arc::new(RwLock::new(HashMap::new())),
            active_listeners: Arc::new(AtomicUsize::new(0)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            track_ended: Arc::new(AtomicBool::new(false)),
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        let mut inner = self.inner.lock();
        
        if inner.broadcast_thread.is_some() {
            warn!("Broadcast thread already exists");
            return;
        }
        
        let music_folder = inner.music_folder.clone();
        let inner_clone = self.inner.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        let should_stop = inner.should_stop.clone();
        
        info!("Starting TRUE RADIO broadcast thread - ONE thread reading for ALL listeners");
        
        let thread_handle = thread::spawn(move || {
            Self::true_radio_broadcast_loop(
                inner_clone,
                broadcast_tx,
                is_streaming,
                track_ended,
                should_stop,
                &music_folder,
            );
        });
        
        inner.broadcast_thread = Some(thread_handle);
        self.is_streaming.store(true, Ordering::SeqCst);
    }
    
    fn true_radio_broadcast_loop(
        inner: Arc<Mutex<StreamManagerInner>>,
        broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        music_folder: &std::path::Path,
    ) {
        info!("TRUE RADIO broadcast thread started - single MP3 reader for all");
        is_streaming.store(true, Ordering::SeqCst);
        
        let mut current_track_index: Option<usize> = None;
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get playlist
            let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                warn!("No tracks in playlist");
                thread::sleep(Duration::from_secs(5));
                continue;
            }
            
            // Get current track
            let track = if let Some(index) = current_track_index {
                playlist.tracks.get(index).cloned()
            } else {
                let index = playlist.current_track.min(playlist.tracks.len() - 1);
                current_track_index = Some(index);
                playlist.tracks.get(index).cloned()
            };
            
            let track = match track {
                Some(track) => track,
                None => {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            
            let track_path = music_folder.join(&track.path);
            
            if !track_path.exists() {
                error!("Track not found: {}", track_path.display());
                current_track_index = Some((current_track_index.unwrap_or(0) + 1) % playlist.tracks.len());
                continue;
            }
            
            info!("BROADCASTING: \"{}\" by {} to ALL listeners", track.title, track.artist);
            
            // Open file for reading
            let mut file = match File::open(&track_path) {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to open track: {}", e);
                    continue;
                }
            };
            
            // Skip ID3 tags
            let id3_size = Self::detect_id3_size(&mut file).unwrap_or(0);
            if id3_size > 0 {
                let _ = file.seek(SeekFrom::Start(id3_size));
            }
            
            // Update inner state
            {
                let mut inner_lock = inner.lock();
                inner_lock.current_file = Some(file.try_clone().unwrap());
                inner_lock.current_track_path = Some(track_path.clone());
                inner_lock.track_start_time = Instant::now();
                inner_lock.playback_position = 0;
                inner_lock.current_track_duration = track.duration;
                
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json);
                }
                
                // Calculate bitrate
                if let Ok(metadata) = std::fs::metadata(&track_path) {
                    let file_size = metadata.len();
                    if track.duration > 0 {
                        inner_lock.current_bitrate = (file_size * 8) / track.duration;
                    }
                }
            }
            
            track_ended.store(false, Ordering::SeqCst);
            
            // BROADCAST LOOP - Read once, send to all
            let start_time = Instant::now();
            let mut chunk_buffer = vec![0u8; BROADCAST_CHUNK_SIZE];
            let mut total_bytes_read = 0u64;
            
            // Calculate timing for smooth playback
            let bytes_per_second = (128000 / 8) as u64; // 128kbps = 16KB/s
            let chunk_duration = Duration::from_millis((BROADCAST_CHUNK_SIZE as u64 * 1000) / bytes_per_second);
            
            info!("Starting broadcast: {}kbps, chunk size: {}B, chunk duration: {:?}", 
                  128, BROADCAST_CHUNK_SIZE, chunk_duration);
            
            loop {
                if should_stop.load(Ordering::SeqCst) {
                    break;
                }
                
                // Read chunk from file
                let bytes_read = match file.read(&mut chunk_buffer) {
                    Ok(0) => {
                        // End of file
                        info!("Track finished broadcasting");
                        break;
                    },
                    Ok(n) => n,
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                };
                
                total_bytes_read += bytes_read as u64;
                
                // Create audio chunk
                let chunk = AudioChunk {
                    data: Bytes::copy_from_slice(&chunk_buffer[..bytes_read]),
                    position: total_bytes_read,
                    timestamp: Instant::now(),
                    chunk_id: {
                        let mut inner_lock = inner.lock();
                        let id = inner_lock.current_chunk_id;
                        inner_lock.current_chunk_id += 1;
                        id
                    },
                };
                
                // Update recent chunks buffer
                {
                    let mut inner_lock = inner.lock();
                    if inner_lock.recent_chunks.len() >= BROADCAST_BUFFER_SIZE {
                        inner_lock.recent_chunks.pop_front();
                    }
                    inner_lock.recent_chunks.push_back(chunk.clone());
                    
                    // Update position
                    let elapsed = start_time.elapsed().as_secs();
                    inner_lock.playback_position = elapsed;
                }
                
                // BROADCAST TO ALL LISTENERS AT ONCE
                match broadcast_tx.send(chunk) {
                    Ok(receiver_count) => {
                        if receiver_count > 0 {
                            debug!("Broadcast chunk to {} listeners", receiver_count);
                        }
                    },
                    Err(_) => {
                        // No receivers, but that's OK
                    }
                }
                
                // Timing control - ensure real-time playback
                let expected_position = Duration::from_secs((total_bytes_read * 8) / 128000);
                let actual_position = start_time.elapsed();
                
                if expected_position > actual_position {
                    let sleep_time = expected_position - actual_position;
                    if sleep_time > Duration::from_millis(1) {
                        thread::sleep(sleep_time);
                    }
                }
            }
            
            // Track ended
            track_ended.store(true, Ordering::SeqCst);
            
            // Move to next track
            if let Some(index) = current_track_index {
                current_track_index = Some((index + 1) % playlist.tracks.len());
                
                // Update playlist
                let mut new_playlist = playlist.clone();
                new_playlist.current_track = current_track_index.unwrap_or(0);
                crate::services::playlist::save_playlist(&new_playlist, &crate::config::PLAYLIST_FILE);
            }
            
            // Brief pause between tracks
            thread::sleep(Duration::from_millis(500));
        }
        
        info!("TRUE RADIO broadcast thread ending");
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
            debug!("ID3v2 tag size: {} bytes", total_size);
            Some(total_size)
        } else {
            Some(0)
        }
    }
    
    // Subscribe a new listener to the broadcast
    pub fn subscribe(&self) -> (String, broadcast::Receiver<AudioChunk>) {
        let connection_id = uuid::Uuid::new_v4().to_string();
        let receiver = self.broadcast_tx.subscribe();
        
        // Add to connections
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
        
        info!("New listener subscribed to broadcast: {}", &connection_id[..8]);
        
        (connection_id, receiver)
    }
    
    // Get recent chunks for late joiners
    pub fn get_recent_chunks(&self, from_chunk_id: u64) -> Vec<AudioChunk> {
        let inner = self.inner.lock();
        inner.recent_chunks
            .iter()
            .filter(|chunk| chunk.chunk_id > from_chunk_id)
            .cloned()
            .collect()
    }
    
    // Connection management methods remain the same
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
        
        connections.retain(|_id, conn_info| {
            let age = now.duration_since(conn_info.last_heartbeat).as_secs();
            age < 30
        });
        
        self.update_listener_count();
    }
    
    pub fn get_active_listeners(&self) -> usize {
        self.connections.read().len()
    }
    
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::SeqCst)
    }
    
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<AudioChunk> {
        self.broadcast_tx.subscribe()
    }
    
    // Get current playback position
    pub fn get_playback_position(&self) -> u64 {
        let inner = self.inner.lock();
        inner.track_start_time.elapsed().as_secs()
    }
    
    pub fn get_precise_position(&self) -> (u64, u64) {
        let inner = self.inner.lock();
        let elapsed = inner.track_start_time.elapsed();
        (elapsed.as_secs(), elapsed.subsec_millis() as u64)
    }
    
    pub fn get_track_info(&self) -> Option<String> {
        self.inner.lock().current_track_info.clone()
    }
    
    pub fn get_current_bitrate(&self) -> u64 {
        self.inner.lock().current_bitrate
    }
    
    pub fn get_current_track_duration(&self) -> u64 {
        self.inner.lock().current_track_duration
    }
    
    pub fn track_ended(&self) -> bool {
        self.track_ended.load(Ordering::SeqCst)
    }
    
    // New methods to fix compilation errors
    pub fn get_track_state(&self) -> TrackState {
        let inner = self.inner.lock();
        let (position_secs, position_ms) = self.get_precise_position();
        let duration = inner.current_track_duration;
        let remaining = if duration > position_secs { duration - position_secs } else { 0 };
        let is_near_end = remaining <= 10; // Last 10 seconds
        
        TrackState {
            position_seconds: position_secs,
            position_milliseconds: position_ms,
            duration,
            remaining_time: remaining,
            is_near_end,
            bitrate: inner.current_bitrate,
            track_info: inner.current_track_info.clone(),
        }
    }
    
    pub fn update_listener_heartbeat(&self, connection_id: &str) {
        let mut connections = self.connections.write();
        if let Some(conn_info) = connections.get_mut(connection_id) {
            conn_info.last_heartbeat = Instant::now();
        }
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping TRUE RADIO broadcast");
        
        self.inner.lock().should_stop.store(true, Ordering::SeqCst);
        self.is_streaming.store(false, Ordering::SeqCst);
        
        let thread = {
            let mut inner = self.inner.lock();
            inner.broadcast_thread.take()
        };
        
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}
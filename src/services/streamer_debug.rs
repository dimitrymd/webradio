// src/services/streamer_debug.rs - Minimal debug version

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

const BROADCAST_CHUNK_SIZE: usize = 4096;
const BROADCAST_BUFFER_SIZE: usize = 50;

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
    broadcast_tx: Arc<broadcast::Sender<AudioChunk>>,
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    active_listeners: Arc<AtomicUsize>,
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    current_track_info: Option<String>,
    playback_position: u64,
    track_start_time: Instant,
    current_chunk_id: u64,
    current_bitrate: u64,
    current_track_duration: u64,
    recent_chunks: VecDeque<AudioChunk>,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing DEBUG StreamManager");
        
        let should_stop = Arc::new(AtomicBool::new(false));
        let now = Instant::now();
        
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE);
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            current_track_info: None,
            playback_position: 0,
            track_start_time: now,
            current_chunk_id: 0,
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
            should_stop,
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        info!("DEBUG: Starting minimal broadcast thread");
        
        let should_stop = self.should_stop.clone();
        let is_streaming = self.is_streaming.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let inner = self.inner.clone();
        
        thread::spawn(move || {
            info!("DEBUG: Broadcast thread started");
            is_streaming.store(true, Ordering::SeqCst);
            
            let mut counter = 0u64;
            
            while !should_stop.load(Ordering::SeqCst) {
                counter += 1;
                
                if counter % 100 == 0 {
                    info!("DEBUG: Broadcast loop iteration {}", counter);
                }
                
                // Create dummy audio chunk
                let dummy_data = vec![0u8; BROADCAST_CHUNK_SIZE];
                let chunk = AudioChunk {
                    data: Bytes::from(dummy_data),
                    position: counter * BROADCAST_CHUNK_SIZE as u64,
                    timestamp: Instant::now(),
                    chunk_id: counter,
                };
                
                // Update inner state
                {
                    let mut inner_lock = inner.lock();
                    inner_lock.current_chunk_id = counter;
                    inner_lock.playback_position = counter;
                    
                    if inner_lock.recent_chunks.len() >= BROADCAST_BUFFER_SIZE {
                        inner_lock.recent_chunks.pop_front();
                    }
                    inner_lock.recent_chunks.push_back(chunk.clone());
                    
                    // Set dummy track info
                    if inner_lock.current_track_info.is_none() {
                        inner_lock.current_track_info = Some(r#"{"title":"Debug Track","artist":"Test Artist","album":"Debug Album","duration":180,"path":"debug.mp3"}"#.to_string());
                        inner_lock.current_track_duration = 180;
                    }
                }
                
                // Broadcast
                match broadcast_tx.send(chunk) {
                    Ok(receiver_count) => {
                        if receiver_count > 0 {
                            debug!("DEBUG: Broadcast to {} listeners", receiver_count);
                        }
                    },
                    Err(_) => {
                        // No receivers
                    }
                }
                
                // Sleep to simulate real-time playback
                thread::sleep(Duration::from_millis(100));
                
                // Stop after 1000 iterations for testing
                if counter >= 1000 {
                    info!("DEBUG: Stopping after 1000 iterations");
                    break;
                }
            }
            
            info!("DEBUG: Broadcast thread ending");
        });
        
        self.is_streaming.store(true, Ordering::SeqCst);
        info!("DEBUG: Broadcast thread started successfully");
    }
    
    // All the required methods
    pub fn subscribe(&self) -> (String, broadcast::Receiver<AudioChunk>) {
        let connection_id = uuid::Uuid::new_v4().to_string();
        let receiver = self.broadcast_tx.subscribe();
        
        {
            let mut connections = self.connections.write();
            connections.insert(connection_id.clone(), ConnectionInfo {
                connected_at: Instant::now(),
                last_heartbeat: Instant::now(),
                platform: "debug".to_string(),
                last_chunk_id: 0,
            });
        }
        
        self.active_listeners.store(self.connections.read().len(), Ordering::SeqCst);
        info!("DEBUG: New listener: {}", &connection_id[..8]);
        
        (connection_id, receiver)
    }
    
    pub fn get_recent_chunks(&self, _from_chunk_id: u64) -> Vec<AudioChunk> {
        let inner = self.inner.lock();
        inner.recent_chunks.iter().cloned().collect()
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
        self.active_listeners.store(self.connections.read().len(), Ordering::SeqCst);
        info!("DEBUG: Removed listener: {}", &connection_id[..8]);
    }
    
    pub fn update_connection_info(&self, connection_id: &str, platform: String, _user_agent: String) {
        let mut connections = self.connections.write();
        if let Some(conn_info) = connections.get_mut(connection_id) {
            conn_info.platform = platform;
        }
    }
    
    pub fn cleanup_stale_connections(&self) {
        let now = Instant::now();
        let mut connections = self.connections.write();
        
        let before_count = connections.len();
        connections.retain(|_id, conn_info| {
            now.duration_since(conn_info.last_heartbeat).as_secs() < 30
        });
        let after_count = connections.len();
        
        if before_count != after_count {
            info!("DEBUG: Cleaned up {} stale connections", before_count - after_count);
        }
        
        self.active_listeners.store(after_count, Ordering::SeqCst);
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
    
    pub fn get_track_state(&self) -> TrackState {
        let inner = self.inner.lock();
        let (position_secs, position_ms) = self.get_precise_position();
        let duration = inner.current_track_duration;
        let remaining = if duration > position_secs { duration - position_secs } else { 0 };
        let is_near_end = remaining <= 10;
        
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
        info!("DEBUG: Stopping broadcast");
        self.should_stop.store(true, Ordering::SeqCst);
        self.is_streaming.store(false, Ordering::SeqCst);
    }
}
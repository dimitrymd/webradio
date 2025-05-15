// Fixed streamer.rs with properly defined constants and improved buffer management

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, error, warn, debug};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, AtomicUsize, AtomicU64, Ordering};

// Fixed missing constants
const BROADCAST_CHUNK_SIZE: usize = 16384; // 16KB chunks
const BROADCAST_RATE_LIMITER_MS: u64 = 10;
const MAX_RECENT_CHUNKS: usize = 50; // Number of chunks to save for new clients
const BROADCAST_BUFFER_SIZE: usize = 50; // Number of chunks to buffer ahead
const MIN_BUFFER_CHUNKS: usize = 10; // Minimum chunks to buffer before starting playback

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
    broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
    active_listeners: Arc<AtomicUsize>,
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    chunk_size: usize,
    
    // Current track info
    current_track_path: Option<String>,
    current_track_info: Option<String>,
    
    // Playback position
    playback_position: u64,
    track_start_time: Instant,
    
    // ID3 header for current track
    id3_header: Option<Vec<u8>>,
    
    // Reference to broadcast sender
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    
    // Recent chunks for new clients
    saved_chunks: VecDeque<Vec<u8>>,
    max_saved_chunks: usize,
    
    // Single broadcast thread - Option to allow stopping/starting
    broadcast_thread: Option<thread::JoinHandle<()>>,
    
    // Control flag for the broadcast thread
    should_stop: Arc<AtomicBool>,
}

// Complete StreamManager implementation with all required methods
impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager");
        
        // Larger buffer for smoother streaming
        let (broadcast_tx, _) = broadcast::channel(1000);
        let should_stop = Arc::new(AtomicBool::new(false));
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            chunk_size,
            current_track_path: None,
            current_track_info: None,
            playback_position: 0,
            track_start_time: Instant::now(),
            id3_header: None,
            broadcast_tx: broadcast_tx.clone(),
            saved_chunks: VecDeque::with_capacity(MAX_RECENT_CHUNKS),
            max_saved_chunks: MAX_RECENT_CHUNKS,
            broadcast_thread: None,
            should_stop: should_stop.clone(),
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
            broadcast_tx: Arc::new(broadcast_tx),
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
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        let should_stop = inner.should_stop.clone();
        
        info!("Starting broadcast thread");
        
        let thread_handle = thread::spawn(move || {
            Self::broadcast_thread_loop(
                inner_clone,
                is_streaming,
                track_ended,
                should_stop,
                &music_folder,
            );
        });
        
        inner.broadcast_thread = Some(thread_handle);
        self.is_streaming.store(true, Ordering::SeqCst);
    }
    
    fn broadcast_thread_loop(
        inner: Arc<Mutex<StreamManagerInner>>,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        music_folder: &Path,
    ) {
        info!("Broadcast thread started");
        is_streaming.store(true, Ordering::SeqCst);
        
        let mut current_track_index: Option<usize> = None;
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get current playlist state
            let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
            
            // Determine which track to play
            let track_to_play = if let Some(index) = current_track_index {
                // We have a known index, use it
                playlist.tracks.get(index).cloned()
            } else {
                // First time or reset, use playlist's current track
                let index = playlist.current_track;
                current_track_index = Some(index);
                playlist.tracks.get(index).cloned()
            };
            
            let track = match track_to_play {
                Some(track) => track,
                None => {
                    warn!("No track at index {:?}", current_track_index);
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            
            let track_path = music_folder.join(&track.path);
            info!("Broadcasting track {}: {} by {}", 
                 current_track_index.unwrap_or(0), track.title, track.artist);
            
            // Update track info
            {
                let mut inner_lock = inner.lock();
                inner_lock.current_track_path = Some(track.path.clone());
                inner_lock.track_start_time = Instant::now();
                inner_lock.playback_position = 0;
                inner_lock.saved_chunks.clear(); // Clear old chunks
                
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json.clone());
                    let _ = inner_lock.broadcast_tx.send(track_json.into_bytes());
                }
            }
            
            // Reset track ended flag
            track_ended.store(false, Ordering::SeqCst);
            
            // Broadcast the track
            Self::broadcast_single_track(
                &inner,
                &track_path,
                &track,
                is_streaming.clone(),
                track_ended.clone(),
                should_stop.clone(),
            );
            
            // Track has ended
            if !should_stop.load(Ordering::SeqCst) {
                info!("Track {} finished", track.title);
                
                // Send transition marker
                if let Some(mut inner_lock) = inner.try_lock() {
                    let _ = inner_lock.broadcast_tx.send(vec![0xFF, 0xFE]);
                    // Clear buffer to ensure clean transition
                    inner_lock.saved_chunks.clear();
                }
                
                // Move to next track
                if let Some(index) = current_track_index {
                    if !playlist.tracks.is_empty() {
                        let next_index = (index + 1) % playlist.tracks.len();
                        current_track_index = Some(next_index);
                        info!("Moving to track index: {}", next_index);
                        
                        // Update playlist file to reflect current position
                        let mut new_playlist = playlist.clone();
                        new_playlist.current_track = next_index;
                        crate::services::playlist::save_playlist(
                            &new_playlist, 
                            &crate::config::PLAYLIST_FILE
                        );
                    }
                }
                
                // Brief pause between tracks
                thread::sleep(Duration::from_millis(500));
            }
        }
        
        info!("Broadcast thread ending");
    }
    
    fn broadcast_single_track(
        inner: &Arc<Mutex<StreamManagerInner>>,
        file_path: &Path,
        track: &crate::models::playlist::Track,
        _is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
    ) {
        let track_start = Instant::now();
        info!("Broadcasting: {} ({}s)", track.title, track.duration);
        
        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Error opening file {}: {}", file_path.display(), e);
                track_ended.store(true, Ordering::SeqCst);
                return;
            }
        };
        
        // Read and send ID3 header
        let mut id3_buffer = vec![0; 4096];
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.id3_header = Some(id3_data.clone());
                    let _ = inner_lock.broadcast_tx.send(id3_data);
                    inner_lock.saved_chunks.push_back(vec![]); // Separator
                }
                
                let _ = file.seek(SeekFrom::Start(0));
            },
            _ => {
                error!("Failed to read ID3 header");
                track_ended.store(true, Ordering::SeqCst);
                return;
            }
        }
        
        // Calculate streaming parameters
        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let bitrate = if track.duration > 0 && file_size > 0 {
            (file_size * 8) / track.duration
        } else {
            128000 // Default to 128kbps if we can't calculate
        };
        
        // Adaptive timing based on bitrate
        let bytes_per_second = bitrate / 8;
        let chunk_duration_ms = (BROADCAST_CHUNK_SIZE as f64 * 1000.0) / bytes_per_second as f64;
        let target_delay = Duration::from_millis(chunk_duration_ms as u64);
        
        info!("Bitrate: {}kbps, chunk delay: {}ms", bitrate/1000, target_delay.as_millis());
        
        // Create initial buffer
        let mut buffer = vec![0; BROADCAST_CHUNK_SIZE];
        let mut chunk_buffer: VecDeque<Vec<u8>> = VecDeque::with_capacity(BROADCAST_BUFFER_SIZE);
        let mut bytes_read_total = 0;
        let mut chunks_sent = 0;
        
        // Fill initial buffer
        info!("Pre-buffering {} chunks...", MIN_BUFFER_CHUNKS);
        while chunk_buffer.len() < MIN_BUFFER_CHUNKS {
            match file.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    chunk_buffer.push_back(buffer[..n].to_vec());
                    bytes_read_total += n;
                },
                Err(e) => {
                    error!("Error during pre-buffering: {}", e);
                    break;
                }
            }
        }
        
        let mut last_send_time = Instant::now();
        let mut file_finished = false;
        
        // Main streaming loop
        while !should_stop.load(Ordering::SeqCst) && !track_ended.load(Ordering::SeqCst) {
            // Keep buffer filled
            while chunk_buffer.len() < BROADCAST_BUFFER_SIZE && !file_finished {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        file_finished = true;
                        break;
                    },
                    Ok(n) => {
                        chunk_buffer.push_back(buffer[..n].to_vec());
                        bytes_read_total += n;
                    },
                    Err(e) => {
                        error!("Error reading file: {}", e);
                        file_finished = true;
                        break;
                    }
                }
            }
            
            // Send chunk if available
            if let Some(chunk) = chunk_buffer.pop_front() {
                if let Some(mut inner_lock) = inner.try_lock() {
                    let elapsed = track_start.elapsed().as_secs();
                    inner_lock.playback_position = elapsed;
                    
                    // Save for late joiners
                    inner_lock.saved_chunks.push_back(chunk.clone());
                    while inner_lock.saved_chunks.len() > MAX_RECENT_CHUNKS {
                        inner_lock.saved_chunks.pop_front();
                    }
                    
                    // Broadcast
                    let _ = inner_lock.broadcast_tx.send(chunk);
                    
                    if chunks_sent % 100 == 0 {
                        info!("Sent {} chunks, buffer: {}, pos: {}s", 
                              chunks_sent, chunk_buffer.len(), elapsed);
                    }
                }
                
                chunks_sent += 1;
                
                // Adaptive timing
                let elapsed_since_last = last_send_time.elapsed();
                if elapsed_since_last < target_delay {
                    thread::sleep(target_delay - elapsed_since_last);
                }
                last_send_time = Instant::now();
            } else if file_finished {
                // No more data
                break;
            } else {
                // Buffer underrun - wait a bit
                warn!("Buffer underrun, waiting...");
                thread::sleep(Duration::from_millis(100));
            }
        }
        
        // Ensure track plays for full duration
        let elapsed = track_start.elapsed().as_secs();
        if track.duration > 0 && elapsed < track.duration {
            let wait_time = track.duration - elapsed;
            info!("Waiting {}s to complete track duration", wait_time);
            thread::sleep(Duration::from_secs(wait_time));
        }
        
        info!("Track {} finished after {}s", track.title, track_start.elapsed().as_secs());
        track_ended.store(true, Ordering::SeqCst);
        
        // Send end marker
        if let Some(mut inner_lock) = inner.try_lock() {
            let _ = inner_lock.broadcast_tx.send(vec![0xFF, 0xFF]);
        }
    }
    
    // Connection management
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.broadcast_tx.subscribe()
    }
    
    pub fn get_chunks_from_current_position(&self) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        let guard = self.inner.lock();
        let header = guard.id3_header.clone();
        let saved_chunks: Vec<Vec<u8>> = guard.saved_chunks.iter().cloned().collect();
        (header, saved_chunks)
    }
    
    pub fn get_track_info(&self) -> Option<String> {
        self.inner.lock().current_track_info.clone()
    }
    
    pub fn get_active_listeners(&self) -> usize {
        self.active_listeners.load(Ordering::SeqCst)
    }
    
    pub fn increment_listener_count(&self) {
        let new_count = self.active_listeners.fetch_add(1, Ordering::SeqCst) + 1;
        info!("Listener connected. Active: {}", new_count);
    }
    
    pub fn decrement_listener_count(&self) {
        let prev = self.active_listeners.load(Ordering::SeqCst);
        if prev > 0 {
            let new_count = self.active_listeners.fetch_sub(1, Ordering::SeqCst) - 1;
            info!("Listener disconnected. Active: {}", new_count);
        }
    }
    
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::SeqCst)
    }
    
    pub fn track_ended(&self) -> bool {
        self.track_ended.load(Ordering::SeqCst)
    }
    
    pub fn get_playback_position(&self) -> u64 {
        self.inner.lock().playback_position
    }
    
    pub fn inner(&self) -> &Self {
        self
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping broadcast");
        
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
    
    // Add the methods that caused the compilation errors
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    pub fn get_saved_chunks_count(&self) -> usize {
        self.inner.lock().saved_chunks.len()
    }
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        // Only stop if explicitly requested
        if self.inner.lock().should_stop.load(Ordering::SeqCst) {
            self.stop_broadcasting();
        }
    }
}
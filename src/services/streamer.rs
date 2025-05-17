// Updated streamer.rs with fixed track transitions

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, error, warn};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::services::transcoder::TranscoderManager;
use crate::config;

// Buffer management constants
const IMPROVED_MAX_RECENT_CHUNKS: usize = 300;
const IMPROVED_MIN_BUFFER_CHUNKS: usize = 50;
const IMPROVED_BROADCAST_BUFFER_SIZE: usize = 100;
const IMPROVED_READ_CHUNK_SIZE: usize = 32 * 1024;

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
    
    // Current bitrate - helps with adaptive buffering
    current_bitrate: u64,

    playback_bytes_position: u64,
    total_track_bytes: u64,
    next_track_buffer: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager with chunk_size={}, buffer_size={}", chunk_size, buffer_size);
        
        let (broadcast_tx, _) = broadcast::channel(IMPROVED_BROADCAST_BUFFER_SIZE); 
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
            saved_chunks: VecDeque::with_capacity(IMPROVED_MAX_RECENT_CHUNKS),
            max_saved_chunks: IMPROVED_MAX_RECENT_CHUNKS,
            broadcast_thread: None,
            should_stop: should_stop.clone(),
            current_bitrate: 128000, // Default starting bitrate
            
            // Initialize new fields
            playback_bytes_position: 0,
            total_track_bytes: 0,
            next_track_buffer: Arc::new(Mutex::new(VecDeque::new())),
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
                {
                    let mut inner_lock = inner.lock(); // Use proper locking instead of try_lock()
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
    ) -> Result<(), std::io::Error> {
        let track_start = Instant::now();
        info!("Broadcasting: {} ({}s)", track.title, track.duration);
        
        let mut file = File::open(file_path)?;
        
        // Read and send ID3 header
        let mut id3_buffer = vec![0; 16384];
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.id3_header = Some(id3_data.clone());
                    let _ = inner_lock.broadcast_tx.send(id3_data);
                    inner_lock.saved_chunks.push_back(vec![]); // Separator
                }
                
                // Rewind to start
                file.seek(SeekFrom::Start(0))?;
            },
            _ => {
                error!("Failed to read ID3 header");
                // Continue anyway - not fatal
            }
        }
        
        // Calculate streaming parameters
        let file_size = file.metadata()?.len();
        let bitrate = if track.duration > 0 && file_size > 0 {
            (file_size * 8) / track.duration
        } else {
            128000 // Default to 128kbps if we can't calculate
        };
        
        // Store bitrate and file size for adaptive buffering
        if let Some(mut inner_lock) = inner.try_lock() {
            inner_lock.current_bitrate = bitrate;
            inner_lock.total_track_bytes = file_size;
            inner_lock.playback_bytes_position = 0;
        }
        
        // Calculate timing parameters
        let bytes_per_second = bitrate / 8;
        let chunk_size = IMPROVED_READ_CHUNK_SIZE;
        let chunk_duration_ms = (chunk_size as f64 * 1000.0) / bytes_per_second as f64;
        let target_delay = Duration::from_millis(chunk_duration_ms as u64);
        
        info!("Bitrate: {}kbps, chunk delay: {}ms", bitrate/1000, target_delay.as_millis());
        
        // Create initial buffer
        let mut buffer = vec![0; chunk_size];
        let mut chunk_buffer: VecDeque<Vec<u8>> = VecDeque::new();
        let mut _bytes_processed = 0; // Using underscore to mark as intentionally unused
        let mut chunks_sent = 0;
        
        // Fill initial buffer
        info!("Pre-buffering {} chunks...", IMPROVED_MIN_BUFFER_CHUNKS);
        while chunk_buffer.len() < IMPROVED_MIN_BUFFER_CHUNKS {
            match file.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    chunk_buffer.push_back(buffer[..n].to_vec());
                    _bytes_processed += n;
                },
                Err(e) => {
                    error!("Error during pre-buffering: {}", e);
                    break;
                }
            }
        }
        
        // Use pre-calculated times for more accurate timing
        let mut last_send_time = Instant::now();
        let mut file_finished = false;
        let mut is_prebuffering = true;
        
        // Main streaming loop
        while !should_stop.load(Ordering::SeqCst) && !track_ended.load(Ordering::SeqCst) {
            // Keep buffer filled
            while chunk_buffer.len() < IMPROVED_MIN_BUFFER_CHUNKS * 2 && !file_finished {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        file_finished = true;
                        break;
                    },
                    Ok(n) => {
                        chunk_buffer.push_back(buffer[..n].to_vec());
                        _bytes_processed += n;
                    },
                    Err(e) => {
                        error!("Error reading file: {}", e);
                        file_finished = true;
                        break;
                    }
                }
            }
            
            // Initial prebuffering - wait until we have a good buffer
            if is_prebuffering {
                if chunk_buffer.len() >= IMPROVED_MIN_BUFFER_CHUNKS {
                    info!("Initial buffer filled with {} chunks, starting playback", chunk_buffer.len());
                    is_prebuffering = false;
                } else if file_finished {
                    // File is smaller than our target buffer
                    info!("File is smaller than target buffer, starting playback");
                    is_prebuffering = false;
                } else {
                    // Keep filling buffer
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            }
            
            // Send chunk if available
            if let Some(chunk) = chunk_buffer.pop_front() {
                if let Some(mut inner_lock) = inner.try_lock() {
                    // Update playback metrics
                    let elapsed = track_start.elapsed().as_secs();
                    inner_lock.playback_position = elapsed;
                    inner_lock.playback_bytes_position += chunk.len() as u64;
                    
                    // Save for late joiners (only save non-empty chunks)
                    if !chunk.is_empty() {
                        inner_lock.saved_chunks.push_back(chunk.clone());
                        while inner_lock.saved_chunks.len() > inner_lock.max_saved_chunks {
                            inner_lock.saved_chunks.pop_front();
                        }
                    }
                    
                    // Broadcast
                    let _ = inner_lock.broadcast_tx.send(chunk);
                    
                    if chunks_sent % 100 == 0 {
                        info!("Sent {} chunks, buffer: {}, pos: {}s", 
                             chunks_sent, chunk_buffer.len(), elapsed);
                    }
                }
                
                chunks_sent += 1;
                
                // Adaptive timing with precision
                let send_time = Instant::now();
                let elapsed_since_last = send_time.duration_since(last_send_time);
                
                if elapsed_since_last < target_delay {
                    let sleep_time = target_delay - elapsed_since_last;
                    thread::sleep(sleep_time);
                }
                
                // Update last send time
                last_send_time = Instant::now();
                
            } else if file_finished {
                // No more data
                break;
            } else {
                // Buffer underrun
                warn!("Buffer underrun, waiting...");
                thread::sleep(Duration::from_millis(config::UNDERRUN_RECOVERY_DELAY_MS));
            }
        }
        
        // Ensure track plays for full duration
        let elapsed = track_start.elapsed().as_secs();
        if track.duration > 0 && elapsed < track.duration && !should_stop.load(Ordering::SeqCst) {
            let wait_time = track.duration - elapsed;
            info!("Waiting {}s to complete track duration", wait_time);
            
            // Use a responsive wait loop that checks should_stop regularly
            let wait_start = Instant::now();
            while wait_start.elapsed().as_secs() < wait_time && !should_stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
            }
        }
        
        info!("Track {} finished after {}s", track.title, track_start.elapsed().as_secs());
        track_ended.store(true, Ordering::SeqCst);
        
        // Send end marker
        if let Some(inner_lock) = inner.try_lock() {
            let _ = inner_lock.broadcast_tx.send(vec![0xFF, 0xFF]);
        }
        
        Ok(())
    }
    
    // Helper function to get the next track
    fn try_get_next_track(inner: &Arc<Mutex<StreamManagerInner>>) -> Option<crate::models::playlist::Track> {
        let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
        
        if playlist.tracks.is_empty() {
            return None;
        }
        
        // Get current track index
        let current_track_path = inner.lock().current_track_path.clone()?;
        
        // Find the current track index
        let current_index = playlist.tracks.iter()
            .position(|t| t.path == current_track_path)?;
        
        // Get next track, wrapping around if necessary
        let next_index = (current_index + 1) % playlist.tracks.len();
        playlist.tracks.get(next_index).cloned()
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
    
    pub fn get_current_bitrate(&self) -> u64 {
        self.inner.lock().current_bitrate
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

    // Set up a connection to feed MP3 data to the transcoder 
    pub fn connect_transcoder(&self, transcoder: Arc<TranscoderManager>) {
        let broadcast_tx = self.broadcast_tx.clone();
        
        thread::spawn(move || {
            info!("Starting MP3 to transcoder feed");
            
            let mut broadcast_rx = broadcast_tx.subscribe();
            
            loop {
                match broadcast_rx.blocking_recv() {
                    Ok(chunk) => {
                        transcoder.add_mp3_chunk(&chunk);
                    },
                    Err(e) => {
                        error!("Error receiving from broadcast: {:?}", e);
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
    }

    // Get playback percentage
    pub fn get_playback_percentage(&self) -> u8 {
        let inner = self.inner.lock();
        
        if inner.total_track_bytes > 0 {
            let percentage = (inner.playback_bytes_position * 100) / inner.total_track_bytes;
            std::cmp::min(percentage as u8, 100)
        } else {
            if let Some(track) = crate::services::playlist::get_current_track(
                &crate::config::PLAYLIST_FILE, 
                &crate::config::MUSIC_FOLDER
            ) {
                if track.duration > 0 {
                    let position = self.get_playback_position();
                    let percentage = (position * 100) / track.duration;
                    std::cmp::min(percentage as u8, 100)
                } else {
                    0
                }
            } else {
                0
            }
        }
    }
    
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    pub fn get_saved_chunks_count(&self) -> usize {
        self.inner.lock().saved_chunks.len()
    }
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        if self.inner.lock().should_stop.load(Ordering::SeqCst) {
            self.stop_broadcasting();
        }
    }
}
// src/streamer.rs - Improved with better track transitions and buffering

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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::config;

// Improved buffer management constants
const MAX_RECENT_CHUNKS: usize = 500;    // Increased for better recovery
const MIN_BUFFER_CHUNKS: usize = 100;    // More pre-buffering
const BROADCAST_BUFFER_SIZE: usize = 300; // Larger buffer
const READ_CHUNK_SIZE: usize = 64 * 1024; // Larger chunks for efficiency
const SMALL_CHUNK_SIZE: usize = 16 * 1024; // Smaller chunks for many listeners

// Track transition constants
const TRANSITION_HEADER: [u8; 2] = [0xFF, 0xFE]; // Track transition marker
const TRACK_END_MARKER: [u8; 2] = [0xFF, 0xFF];  // Track end marker

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

    // Position tracking
    playback_bytes_position: u64,
    total_track_bytes: u64,
    
    // Next track buffering
    next_track_buffer: Arc<Mutex<VecDeque<Vec<u8>>>>,
    next_track_header: Arc<Mutex<Option<Vec<u8>>>>,
    next_track_path: Arc<Mutex<Option<PathBuf>>>,
    
    // Error tracking and recovery
    last_error: Arc<Mutex<Option<String>>>,
    error_count: AtomicUsize,
    last_reset_time: Instant,
}

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager with chunk_size={}, buffer_size={}", chunk_size, buffer_size);
        
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_BUFFER_SIZE); 
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
            current_bitrate: 128000, // Default starting bitrate
            
            // Position tracking
            playback_bytes_position: 0,
            total_track_bytes: 0,
            
            // New fields for track transitions
            next_track_buffer: Arc::new(Mutex::new(VecDeque::new())),
            next_track_header: Arc::new(Mutex::new(None)),
            next_track_path: Arc::new(Mutex::new(None)),
            
            // Error tracking
            last_error: Arc::new(Mutex::new(None)),
            error_count: AtomicUsize::new(0),
            last_reset_time: Instant::now(),
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
            
            // Check if playlist is empty
            if playlist.tracks.is_empty() {
                warn!("Playlist is empty, waiting for tracks...");
                thread::sleep(Duration::from_secs(5));
                continue;
            }
            
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
                    warn!("No track at index {:?}, resetting to 0", current_track_index);
                    current_track_index = Some(0);
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            
            let track_path = music_folder.join(&track.path);
            info!("Broadcasting track {}: {} by {}", 
                 current_track_index.unwrap_or(0), track.title, track.artist);
            
            // Update track info with proper locking
            {
                let mut inner_lock = inner.lock();
                inner_lock.current_track_path = Some(track.path.clone());
                inner_lock.track_start_time = Instant::now();
                inner_lock.playback_position = 0;
                inner_lock.playback_bytes_position = 0;
                
                // Do not clear saved chunks here to provide continuous playback!
                
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json.clone());
                }
            }
            
            // Reset track ended flag
            track_ended.store(false, Ordering::SeqCst);
            
            // Check if we should pre-fetch the next track
            let next_track_index = (current_track_index.unwrap_or(0) + 1) % playlist.tracks.len();
            if let Some(next_track) = playlist.tracks.get(next_track_index) {
                let next_track_path = music_folder.join(&next_track.path);
                
                // Store next track path for potential pre-buffering
                {
                    let mut next_track_path_lock = inner.lock().next_track_path.lock();
                    *next_track_path_lock = Some(next_track_path.clone());
                }
            }
            
            // Broadcast the track
            let broadcast_result = Self::broadcast_single_track(
                &inner,
                &track_path,
                &track,
                is_streaming.clone(),
                track_ended.clone(),
                should_stop.clone(),
            );
            
            // Track broadcasting finished or encountered an error
            if let Err(e) = broadcast_result {
                // Log error and update error tracking
                error!("Error broadcasting track: {}", e);
                {
                    let mut last_error = inner.lock().last_error.lock();
                    *last_error = Some(format!("Error broadcasting track: {}", e));
                }
                
                // Increment error count
                inner.lock().error_count.fetch_add(1, Ordering::SeqCst);
            }
            
            // Track has ended
            if !should_stop.load(Ordering::SeqCst) {
                info!("Track {} finished", track.title);
                
                // Send transition marker
                {
                    let inner_lock = inner.lock();
                    let _ = inner_lock.broadcast_tx.send(TRANSITION_HEADER.to_vec());
                    
                    // Do NOT clear saved chunks! Keep them for late joiners
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
                thread::sleep(Duration::from_millis(200));
            }
        }
        
        info!("Broadcast thread ending");
    }
    
    fn broadcast_single_track(
        inner: &Arc<Mutex<StreamManagerInner>>,
        file_path: &Path,
        track: &crate::models::playlist::Track,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
    ) -> Result<(), std::io::Error> {
        let track_start = Instant::now();
        info!("Broadcasting: {} ({}s)", track.title, track.duration);
        
        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Failed to open file {}: {}", file_path.display(), e);
                return Err(e);
            }
        };
        
        // Check if we have a pre-buffered next track header and data
        // This is used for the "next" track, but we're playing "this" track now
        // So we need to clear any previous pre-buffering for the "next" track
        {
            let inner_lock = inner.lock();
            let mut next_header = inner_lock.next_track_header.lock();
            let mut next_buffer = inner_lock.next_track_buffer.lock();
            *next_header = None;
            next_buffer.clear();
        }
        
        // Read and send ID3 header
        let mut id3_buffer = vec![0; 16384];
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                // Get lock and update state
                let mut inner_lock = inner.lock();
                inner_lock.id3_header = Some(id3_data.clone());
                
                // Send header to clients
                let _ = inner_lock.broadcast_tx.send(id3_data);
                
                // Store empty chunk as separator in saved chunks
                inner_lock.saved_chunks.push_back(vec![]);
                
                // Rewind to start
                file.seek(SeekFrom::Start(0))?;
            },
            _ => {
                error!("Failed to read ID3 header from {}", file_path.display());
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
        {
            let mut inner_lock = inner.lock();
            inner_lock.current_bitrate = bitrate;
            inner_lock.total_track_bytes = file_size;
            inner_lock.playback_bytes_position = 0;
        }
        
        // Calculate timing parameters
        let bytes_per_second = bitrate / 8;
        let chunk_size = READ_CHUNK_SIZE;
        let chunk_duration_ms = (chunk_size as f64 * 1000.0) / bytes_per_second as f64;
        let target_delay = Duration::from_millis(chunk_duration_ms as u64);
        
        info!("Bitrate: {}kbps, chunk delay: {}ms", bitrate/1000, target_delay.as_millis());
        
        // Create initial buffer
        let mut buffer = vec![0; chunk_size];
        let mut chunk_buffer: VecDeque<Vec<u8>> = VecDeque::new();
        let mut bytes_processed = 0;
        let mut chunks_sent = 0;
        
        // Fill initial buffer
        info!("Pre-buffering {} chunks...", MIN_BUFFER_CHUNKS);
        while chunk_buffer.len() < MIN_BUFFER_CHUNKS {
            match file.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    chunk_buffer.push_back(buffer[..n].to_vec());
                    bytes_processed += n;
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
        
        // Look ahead to pre-buffer the next track when we reach 80% of this track
        let mut next_track_prebuffered = false;
        
        // Main streaming loop
        while !should_stop.load(Ordering::SeqCst) && !track_ended.load(Ordering::SeqCst) {
            // Keep buffer filled
            while chunk_buffer.len() < MIN_BUFFER_CHUNKS * 2 && !file_finished {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        file_finished = true;
                        break;
                    },
                    Ok(n) => {
                        chunk_buffer.push_back(buffer[..n].to_vec());
                        bytes_processed += n;
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
                if chunk_buffer.len() >= MIN_BUFFER_CHUNKS {
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
            
            // Check if we should pre-buffer the next track
            // Do this when we're 80% through the current track
            if !next_track_prebuffered && track.duration > 0 {
                let elapsed = track_start.elapsed().as_secs();
                let percentage = (elapsed * 100) / track.duration;
                
                if percentage >= 80 {
                    // Start pre-buffering the next track
                    if let Some(next_path) = inner.lock().next_track_path.lock().clone() {
                        info!("Pre-buffering next track at {}% of current track", percentage);
                        Self::prefetch_next_track_internal(inner, &next_path);
                        next_track_prebuffered = true;
                    }
                }
            }
            
            // Send chunk if available
            if let Some(chunk) = chunk_buffer.pop_front() {
                // Get lock for updating state
                let inner_lock = inner.lock();
                
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
                
                // Drop lock before sleeping
                std::mem::drop(inner_lock);
                
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
                
                // Update playback position during wait
                inner.lock().playback_position = elapsed + wait_start.elapsed().as_secs();
            }
        }
        
        info!("Track {} finished after {}s", track.title, track_start.elapsed().as_secs());
        track_ended.store(true, Ordering::SeqCst);
        
        // Send end marker
        inner.lock().broadcast_tx.send(TRACK_END_MARKER.to_vec()).ok();
        
        Ok(())
    }
    
    // Public function to pre-buffer a track
    pub fn prefetch_next_track(&self, track_path: &Path) {
        Self::prefetch_next_track_internal(&self.inner, track_path);
    }
    
    // Internal implementation of track pre-buffering
    fn prefetch_next_track_internal(inner: &Arc<Mutex<StreamManagerInner>>, track_path: &Path) {
        info!("Pre-buffering next track: {}", track_path.display());
        
        // Open file to read ID3 header and initial chunks
        match File::open(track_path) {
            Ok(mut file) => {
                // Read ID3 header first
                let mut id3_buffer = vec![0; 16384];
                match file.read(&mut id3_buffer) {
                    Ok(n) if n > 0 => {
                        let id3_data = id3_buffer[..n].to_vec();
                        
                        // Store header for next track
                        let mut next_header = inner.lock().next_track_header.lock();
                        *next_header = Some(id3_data);
                        
                        // Reset file position
                        let _ = file.seek(SeekFrom::Start(0));
                    },
                    _ => {
                        warn!("Failed to read ID3 header for next track");
                    }
                }
                
                // Read initial chunks for the next track
                let mut buffer = vec![0; READ_CHUNK_SIZE];
                let mut prebuffer = VecDeque::new();
                let prebuffer_chunks = 50; // Number of chunks to pre-buffer
                
                for _ in 0..prebuffer_chunks {
                    match file.read(&mut buffer) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            prebuffer.push_back(buffer[..n].to_vec());
                        },
                        Err(e) => {
                            error!("Error pre-buffering next track: {}", e);
                            break;
                        }
                    }
                }
                
                info!("Pre-buffered {} chunks of next track", prebuffer.len());
                
                // Store in shared buffer
                let mut next_buffer = inner.lock().next_track_buffer.lock();
                *next_buffer = prebuffer;
            },
            Err(e) => {
                error!("Failed to open next track for pre-buffering: {}", e);
            }
        }
    }
    
    // Connection management
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.broadcast_tx.subscribe()
    }
    
    // Get chunks from the current position for a new client connection
    pub fn get_chunks_from_current_position(&self) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        let guard = self.inner.lock();
        let header = guard.id3_header.clone();
        
        // First check if we're in a track transition and have pre-buffered data for next track
        let next_header = guard.next_track_header.lock().clone();
        let next_buffer = guard.next_track_buffer.lock().clone();
        
        if self.track_ended.load(Ordering::SeqCst) && next_header.is_some() && !next_buffer.is_empty() {
            // We're transitioning to next track and have pre-buffered data
            debug!("Providing pre-buffered next track data to new client");
            return (next_header, next_buffer.iter().cloned().collect());
        }
        
        // Normal case - provide current track data
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
    
    // Stats
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    pub fn get_saved_chunks_count(&self) -> usize {
        self.inner.lock().saved_chunks.len()
    }
    
    // Manually trigger next track if needed
    pub fn force_next_track(&self) {
        info!("Forcing transition to next track");
        self.track_ended.store(true, Ordering::SeqCst);
        
        // Wait a moment for transition to happen
        thread::sleep(Duration::from_millis(500));
    }
    
    // Restart the broadcast if needed
    pub fn restart_if_needed(&self) {
        if !self.is_streaming() {
            info!("Restarting broadcast thread");
            self.start_broadcast_thread();
        }
    }
    
    // Stop broadcasting
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
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        if !self.inner.lock().should_stop.load(Ordering::SeqCst) {
            self.stop_broadcasting();
        }
    }
}
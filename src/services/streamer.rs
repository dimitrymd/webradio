use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, error, warn, debug};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// Size of chunks for broadcasting
const BROADCAST_CHUNK_SIZE: usize = 8192; // 8KB chunks for broadcasting
const BROADCAST_RATE_LIMITER_MS: u64 = 50; // Control how fast we send chunks to match real-time playback
const MAX_SAVED_CHUNKS: usize = 1000; // Max number of chunks to save for new clients (~8MB)

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
    // Broadcast channel for streaming audio to multiple clients
    broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
    // Atomic counters for stats that can be accessed without locking
    active_listeners: Arc<AtomicUsize>,
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    chunk_size: usize,
    buffer_size: usize,
    cache_time: u64,
    
    // Track currently being streamed
    current_track_path: Option<String>,
    current_track_info: Option<String>, // JSON string with track metadata
    
    // Buffer for audio chunks (now mainly used for recovery)
    buffer: VecDeque<Vec<u8>>,
    
    // Time when each chunk was added
    chunk_times: VecDeque<Instant>,
    
    // Stream state
    stream_thread: Option<JoinHandle<()>>,
    
    // Last buffer update time - used for detecting stalled streams
    last_buffer_update: Instant,
    
    // Current playback position in seconds
    playback_position: u64,
    
    // ID3 header data for MP3 files (to be sent first to each client)
    id3_header: Option<Vec<u8>>,
    
    // Reference to broadcast sender
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    
    // Track start time for real-time position tracking
    track_start_time: Instant,
    
    // Whether to use real-time position tracking
    real_time_position: bool,
    
    // Saved audio data for new clients (circular buffer)
    saved_chunks: VecDeque<Vec<u8>>,
    max_saved_chunks: usize,
}

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, cache_time: u64) -> Self {
        info!("Initializing StreamManager with music_folder: {}, chunk_size: {}, buffer_size: {}, cache_time: {}",
            music_folder.display(), chunk_size, buffer_size, cache_time);
        
        // Create broadcast channel with capacity for 200 messages
        // This allows late joiners to still receive recent data
        let (broadcast_tx, _) = broadcast::channel(200); // Increased capacity
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            chunk_size,
            buffer_size,
            cache_time,
            current_track_path: None,
            current_track_info: None,
            buffer: VecDeque::with_capacity(buffer_size / chunk_size),
            chunk_times: VecDeque::with_capacity(buffer_size / chunk_size),
            stream_thread: None,
            last_buffer_update: Instant::now(),
            playback_position: 0,
            id3_header: None,
            broadcast_tx: broadcast_tx.clone(),
            track_start_time: Instant::now(),
            real_time_position: true, // Use real-time tracking by default
            saved_chunks: VecDeque::with_capacity(MAX_SAVED_CHUNKS),
            max_saved_chunks: MAX_SAVED_CHUNKS,
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
            broadcast_tx: Arc::new(broadcast_tx),
            // Initialize atomic counters
            active_listeners: Arc::new(AtomicUsize::new(0)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            track_ended: Arc::new(AtomicBool::new(false)),
        }
    }
    
    // Prepare for track switching - called before advancing to next track
    pub fn prepare_for_track_switch(&self) {
        // Don't set streaming to false, just mark track as ended
        self.track_ended.store(true, Ordering::SeqCst);
        
        println!("Preparing for track switch - track marked as ended");
        
        // Signal end of track to clients
        let inner = self.inner.lock();
        let _ = inner.broadcast_tx.send(Vec::new());
    }
    
    pub fn start_streaming(&self, track_path: &str) {
        // CRITICAL: Reset ALL state flags at the beginning
        self.is_streaming.store(true, Ordering::SeqCst);
        self.track_ended.store(false, Ordering::SeqCst);
        
        println!("Start streaming - reset streaming flags to correct state");
        
        // Now acquire the mutex for the actual work
        let mut inner = self.inner.lock();
        
        // If already streaming this track, do nothing
        if inner.current_track_path.as_deref() == Some(track_path) && 
           self.is_streaming.load(Ordering::SeqCst) && 
           !self.track_ended.load(Ordering::SeqCst) {
            debug!("Already streaming track: {}", track_path);
            return;
        }
        
        info!("Starting to stream track: {}", track_path);
        
        // Clean up existing stream thread if any
        self.cleanup_stream_thread(&mut inner);
        
        // Clear buffer for the new track
        inner.buffer.clear();
        inner.chunk_times.clear();
        inner.playback_position = 0;
        inner.id3_header = None;
        inner.saved_chunks.clear(); // Clear saved chunks buffer
        debug!("Cleared buffer for new track");
        
        // Set the current track path
        inner.current_track_path = Some(track_path.to_string());
        
        // Prepare track info JSON
        if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE, 
            &crate::config::MUSIC_FOLDER
        ) {
            if let Ok(track_json) = serde_json::to_string(&track) {
                inner.current_track_info = Some(track_json);
            }
        }
        
        // Reset the track start time for real-time position tracking
        inner.track_start_time = Instant::now();
        
        let music_folder = inner.music_folder.clone();
        let track_path = track_path.to_string();
        let chunk_size = BROADCAST_CHUNK_SIZE; // Use broadcast chunk size for streaming
        let inner_clone = self.inner.clone();
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        
        debug!("Creating new stream thread for track: {}", track_path);
        let thread_handle = thread::spawn(move || {
            // Stream the track to all listeners
            Self::buffer_track(inner_clone, is_streaming, track_ended, &music_folder, &track_path, chunk_size);
        });
        
        inner.stream_thread = Some(thread_handle);
    }
    
    // Helper method to clean up existing stream thread
    fn cleanup_stream_thread(&self, inner: &mut StreamManagerInner) {
        // Stop existing stream thread if any
        if self.is_streaming.load(Ordering::SeqCst) && inner.stream_thread.is_some() {
            info!("Stopping existing stream thread");
            self.is_streaming.store(false, Ordering::SeqCst);
            if let Some(thread) = inner.stream_thread.take() {
                // Release lock while joining
                let inner_ptr = Arc::as_ptr(&self.inner);
                drop(inner); 
                
                // Wait for thread to terminate with timeout
                let timeout = Duration::from_secs(5);
                let now = Instant::now();
                
                // Try to join the thread with a timeout
                while now.elapsed() < timeout {
                    if thread.is_finished() {
                        let _ = thread.join();
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                
                // Get the lock back (safely, in case we're already holding it)
                if let Some(inner_ref) = unsafe { inner_ptr.as_ref() } {
                    let _ = inner_ref.lock();
                }
            }
        }
    }
    
    // Non-blocking method to get saved chunks - returns immediately without waiting
    pub fn get_saved_chunks(&self) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        // Use a short timeout for lock acquisition to avoid blocking
        let guard = self.inner.lock();
        
        // Return ID3 header and saved chunks (cloned to avoid holding the lock)
        let header = guard.id3_header.clone();
        let chunks: Vec<Vec<u8>> = guard.saved_chunks.iter().cloned().collect();
        
        (header, chunks)
    }
    
    // Improved buffer_track method that uses atomic flags
    fn buffer_track(
        inner: Arc<Mutex<StreamManagerInner>>, 
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        music_folder: &Path, 
        track_path: &str, 
        chunk_size: usize
    ) {
        let file_path = music_folder.join(track_path);
        let start_time = std::time::Instant::now();
        
        println!("Starting to buffer track: {}", file_path.display());
        
        if !file_path.exists() {
            println!("ERROR: File not found: {}", file_path.display());
            is_streaming.store(false, Ordering::SeqCst);
            track_ended.store(true, Ordering::SeqCst);
            return;
        }
        
        println!("Opening file for streaming: {}", file_path.display());
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                println!("ERROR: Error opening file {}: {}", file_path.display(), e);
                is_streaming.store(false, Ordering::SeqCst);
                track_ended.store(true, Ordering::SeqCst);
                return;
            }
        };
        
        // Get file size and duration for tracking progress
        let file_size = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        };
        
        // Get the expected duration from the playlist information
        let expected_duration = if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE, 
            &crate::config::MUSIC_FOLDER
        ) {
            track.duration
        } else {
            0
        };
        
        println!("Starting to broadcast file: {}, size: {} bytes, expected duration: {} seconds", 
                 file_path.display(), file_size, expected_duration);
        
        // Calculate playback rate based on file size and duration
        let bytes_per_second = if file_size > 0 && expected_duration > 0 {
            file_size / expected_duration
        } else {
            16000 // default to 16KB/s (128kbps)
        };
        
        println!("Calculated streaming rate: {} bytes/second", bytes_per_second);
        
        // Calculate chunk delay based on expected duration
        let bytes_per_chunk = chunk_size as f64;
        let chunks_per_second = bytes_per_second as f64 / bytes_per_chunk;
        let chunk_delay_ms = 1000.0 / chunks_per_second;
        
        // Use at least a minimum delay to avoid overwhelming clients
        let chunk_delay = std::cmp::max(
            Duration::from_millis(BROADCAST_RATE_LIMITER_MS),
            Duration::from_millis(chunk_delay_ms as u64)
        );
        
        println!("Broadcasting with delay of {:.2}ms between chunks", chunk_delay.as_millis());
        
        // Send track info first
        if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE,
            &crate::config::MUSIC_FOLDER
        ) {
            if let Ok(track_info) = serde_json::to_string(&track) {
                println!("Broadcasting track info: {}", track_info);
                
                // Update track info with minimal locking
                {
                    let mut inner_lock = inner.lock();
                    inner_lock.current_track_info = Some(track_info.clone());
                    
                    // Broadcast track info to clients
                    let _ = inner_lock.broadcast_tx.send(track_info.into_bytes());
                }
            }
        }
        
        // Extract and store the ID3 header (first few KB of MP3 file)
        let mut id3_buffer = vec![0; 16384]; // 16KB should be enough for most ID3 headers
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                // Store ID3 header with minimal locking
                {
                    let mut inner_lock = inner.lock();
                    inner_lock.id3_header = Some(id3_data.clone());
                    
                    // Broadcast ID3 header to all listeners
                    let _ = inner_lock.broadcast_tx.send(id3_data);
                }
                
                // Reset file position to beginning
                if let Err(e) = file.seek(SeekFrom::Start(0)) {
                    println!("ERROR: Failed to seek back to beginning of file: {}", e);
                    is_streaming.store(false, Ordering::SeqCst);
                    track_ended.store(true, Ordering::SeqCst);
                    return;
                }
            },
            Ok(0) => {
                println!("WARNING: Empty file: {}", file_path.display());
                is_streaming.store(false, Ordering::SeqCst);
                track_ended.store(true, Ordering::SeqCst);
                return;
            },
            Err(e) => {
                println!("ERROR: Failed to read ID3 header: {}", e);
                is_streaming.store(false, Ordering::SeqCst);
                track_ended.store(true, Ordering::SeqCst);
                return;
            },
            _ => {} // Other cases handled by compiler
        }
        
        // Continue with normal buffering
        let mut buffer = vec![0; chunk_size];
        let mut last_progress_log = std::time::Instant::now();
        let mut total_bytes_read = 0;
        let mut chunks_sent = 0;
        
        // Track real elapsed time since starting
        let real_start_time = Instant::now();
        
        // Track playback position
        let mut last_position_update = Instant::now();
        
        let mut eof_reached = false;
        
        while is_streaming.load(Ordering::SeqCst) && !eof_reached {
            // Update playback position based on real elapsed time (every second)
            if last_position_update.elapsed().as_secs() >= 1 {
                // Calculate position based on bytes read and expected bitrate
                let elapsed_secs = real_start_time.elapsed().as_secs();
                let byte_based_position = if bytes_per_second > 0 {
                    total_bytes_read / bytes_per_second
                } else {
                    elapsed_secs
                };
                
                // Update position with minimal locking
                {
                    let mut inner_lock = inner.lock();
                    inner_lock.playback_position = byte_based_position;
                }
                
                last_position_update = Instant::now();
            }
            
            // Log progress every 5 seconds
            if last_progress_log.elapsed().as_secs() >= 5 {
                // Get position with minimal locking
                let current_position;
                let buffer_len;
                let buffer_capacity;
                let receiver_count;
                
                {
                    let inner_lock = inner.lock();
                    
                    current_position = if inner_lock.real_time_position {
                        let elapsed = real_start_time.elapsed().as_secs();
                        // Cap at track duration if needed
                        if expected_duration > 0 && elapsed > expected_duration {
                            expected_duration
                        } else {
                            elapsed
                        }
                    } else {
                        inner_lock.playback_position
                    };
                    
                    buffer_len = inner_lock.buffer.len();
                    buffer_capacity = inner_lock.buffer.capacity();
                    receiver_count = inner_lock.broadcast_tx.receiver_count();
                }
                
                println!("BUFFER STATUS: Broadcasting \"{}\" - {} bytes read ({:.2}% of file) over {} seconds, position={}s", 
                       track_path, 
                       total_bytes_read,
                       if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 },
                       start_time.elapsed().as_secs(),
                       current_position);
                
                println!("Buffer status: {}/{} chunks ({:.2}%), {} active receivers", 
                       buffer_len, buffer_capacity, 
                       if buffer_capacity > 0 { (buffer_len as f64 / buffer_capacity as f64) * 100.0 } else { 0.0 },
                       receiver_count);
                
                last_progress_log = std::time::Instant::now();
            }
            
            // Read the next chunk
            match file.read(&mut buffer) {
                Ok(0) => {
                    // End of file reached
                    eof_reached = true;
                    
                    // Wait for at least expected_duration before ending the track
                    // This prevents premature track ending due to fast reading
                    let elapsed = start_time.elapsed().as_secs();
                    let position = if bytes_per_second > 0 {
                        total_bytes_read / bytes_per_second
                    } else {
                        elapsed
                    };
                    
                    println!("End of file reached for track: {} after {} seconds, position={}s of expected {}s", 
                           track_path, elapsed, position, expected_duration);
                    
                    // If we need to wait to reach expected duration
                    if expected_duration > 0 && position < expected_duration {
                        let wait_seconds = expected_duration - position;
                        println!("Waiting {} more seconds to complete full track duration", wait_seconds);
                        
                        // Use a much longer sleep interval to reduce log spam and improve efficiency
                        let sleep_interval = 5; // 5 seconds
                        let mut remaining = wait_seconds;
                        
                        while remaining > 0 && is_streaming.load(Ordering::SeqCst) {
                            // Sleep for the interval or the remaining time, whichever is smaller
                            let sleep_time = std::cmp::min(sleep_interval, remaining);
                            thread::sleep(Duration::from_secs(sleep_time));
                            remaining -= sleep_time;
                            
                            // Update position based on elapsed time
                            let new_position = position + (wait_seconds - remaining);
                            
                            // Update with minimal locking
                            {
                                let mut inner_lock = inner.lock();
                                inner_lock.playback_position = new_position;
                            }
                            
                            // Log progress every 30 seconds or at the end
                            if remaining % 30 == 0 || remaining < sleep_interval {
                                println!("Track \"{}\" at position {}s of {}s, {} seconds remaining", 
                                       track_path, new_position, expected_duration, 
                                       if new_position < expected_duration { expected_duration - new_position } else { 0 });
                            }
                        }
                    }
                    
                    println!("Track playback complete after waiting: {} actual seconds", start_time.elapsed().as_secs());
                    
                    // Signal the end of track using atomic flag first
                    track_ended.store(true, Ordering::SeqCst);
                    
                    // IMPORTANT: Don't set is_streaming to false here!
                    // That would cause clients to disconnect instead of waiting for the next track
                    
                    // Then update other state with minimal locking
                    {
                        let mut inner_lock = inner.lock();
                        
                        // Send empty chunk to signal end of track to clients
                        let _ = inner_lock.broadcast_tx.send(Vec::new());
                    }
                    
                    println!("Set track_ended flag for \"{}\" after {} seconds - STREAMING REMAINS ACTIVE", 
                           track_path, start_time.elapsed().as_secs());
                    break;
                },
                Ok(n) => {
                    // Got a chunk of data, broadcast it
                    chunks_sent += 1;
                    total_bytes_read += n as u64;
                    
                    if chunks_sent % 100 == 0 {
                        println!("Sent {} chunks, {} bytes ({:.2}% of file)", 
                               chunks_sent, total_bytes_read,
                               if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 });
                    }
                    
                    // Get the chunk data
                    let chunk_data = buffer[..n].to_vec();
                    
                    // Add to buffer and update stream state with minimal locking
                    {
                        let mut inner_lock = inner.lock();
                        
                        // Add to buffer (for recovery/late joiners)
                        inner_lock.buffer.push_back(chunk_data.clone());
                        inner_lock.chunk_times.push_back(Instant::now());
                        inner_lock.last_buffer_update = Instant::now();
                        
                        // Trim buffer if it gets too large
                        while inner_lock.buffer.len() > inner_lock.buffer.capacity() {
                            inner_lock.buffer.pop_front();
                            inner_lock.chunk_times.pop_front();
                        }
                        
                        // Add to saved chunks for new clients
                        inner_lock.saved_chunks.push_back(chunk_data.clone());
                        
                        // Keep saved chunks within size limit
                        while inner_lock.saved_chunks.len() > inner_lock.max_saved_chunks {
                            inner_lock.saved_chunks.pop_front();
                        }
                        
                        // Broadcast the chunk to all listeners
                        let _ = inner_lock.broadcast_tx.send(chunk_data);
                    }
                    
                    // Sleep to control broadcast rate - this is crucial for real-time playback simulation
                    thread::sleep(chunk_delay);
                },
                Err(e) => {
                    println!("ERROR: Error reading file {}: {}", file_path.display(), e);
                    
                    // Set error state with minimal locking
                    is_streaming.store(false, Ordering::SeqCst);
                    track_ended.store(true, Ordering::SeqCst);
                    
                    // Send end of track signal
                    {
                        let inner_lock = inner.lock();
                        let _ = inner_lock.broadcast_tx.send(Vec::new());
                    }
                    
                    break;
                }
            }
        }
        
        println!("Exiting buffer_track thread for track: {}", track_path);
    }
    
    // Add a method to force next track
    pub fn force_next_track(&self) {
        println!("Forcing switch to next track due to timeout");
        
        // Signal end of track to clients with minimal locking
        {
            let inner = self.inner.lock();
            let _ = inner.broadcast_tx.send(Vec::new());
        }
        
        // Set track ended flag to trigger track switcher
        self.track_ended.store(true, Ordering::SeqCst);
        
        // Make sure streaming flag is still true
        self.is_streaming.store(true, Ordering::SeqCst);
    }

    pub fn force_stop_streaming(&self) {
        // Use atomic flags for quick updates
        self.is_streaming.store(false, Ordering::SeqCst);
        self.track_ended.store(true, Ordering::SeqCst);
        
        // Signal end of track to clients
        let inner = self.inner.lock();
        let _ = inner.broadcast_tx.send(Vec::new());
        
        println!("Force stopped broadcasting by setting streaming and track_ended flags");
    }
    
    // Get a broadcast receiver for clients to listen to the stream
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.broadcast_tx.subscribe()
    }
    
    // Get ID3 header for new connections - non-blocking
    pub fn get_id3_header(&self) -> Option<Vec<u8>> {
        let inner = self.inner.lock();
        inner.id3_header.clone()
    }
    
    // Get current track info - non-blocking
    pub fn get_track_info(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.current_track_info.clone()
    }
    
    // Use atomic counter for fast access without locking
    pub fn get_active_listeners(&self) -> usize {
        self.active_listeners.load(Ordering::SeqCst)
    }
    
    // Use atomic flag for fast access without locking
    pub fn is_streaming(&self) -> bool {
        self.is_streaming.load(Ordering::SeqCst)
    }
    
    // Use atomic flag for fast access without locking
    pub fn track_ended(&self) -> bool {
        self.track_ended.load(Ordering::SeqCst)
    }
    
    // Use atomic counter for fast updates
    pub fn increment_listener_count(&self) {
        let new_count = self.active_listeners.fetch_add(1, Ordering::SeqCst) + 1;
        info!("Listener connected. Active listeners: {}", new_count);
    }

    // Use atomic counter for fast updates
    pub fn decrement_listener_count(&self) {
        let prev_count = self.active_listeners.fetch_sub(1, Ordering::SeqCst);
        if prev_count > 0 {
            info!("Listener disconnected. Active listeners: {}", prev_count - 1);
        } else {
            self.active_listeners.store(0, Ordering::SeqCst);
            info!("No active listeners (attempted to decrement below 0)");
        }
    }
    
    pub fn inner(&self) -> &Self {
        self
    }
    
    // Get current track path - quick access
    pub fn get_current_track_path(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.current_track_path.clone()
    }
    
    // Get playback position - quick access
    pub fn get_playback_position(&self) -> u64 {
        let inner = self.inner.lock();
        
        if inner.real_time_position {
            // Calculate position based on real elapsed time since track started
            let elapsed = inner.track_start_time.elapsed().as_secs();
            
            // Don't exceed track duration
            if let Some(track) = crate::services::playlist::get_current_track(
                &crate::config::PLAYLIST_FILE, 
                &crate::config::MUSIC_FOLDER
            ) {
                if track.duration > 0 && elapsed > track.duration {
                    return track.duration;
                }
            }
            
            return elapsed;
        } else {
            // Return the original position based on data read
            inner.playback_position
        }
    }
    
    pub fn set_real_time_position(&self, enabled: bool) {
        let mut inner = self.inner.lock();
        inner.real_time_position = enabled;
        info!("Real-time position tracking {}", if enabled { "enabled" } else { "disabled" });
    }
    
    pub fn is_real_time_position(&self) -> bool {
        let inner = self.inner.lock();
        inner.real_time_position
    }
    
    pub fn buffer_status(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        (inner.buffer.len(), inner.buffer.capacity())
    }
    
    // Use atomic flag for quick updates
    pub fn reset_track_ended_flag(&self) {
        self.track_ended.store(false, Ordering::SeqCst);
    }
    
    // Check for stalled streams
    pub fn is_stream_stalled(&self) -> bool {
        let inner = self.inner.lock();
        inner.last_buffer_update.elapsed() > Duration::from_secs(10) && self.is_streaming.load(Ordering::SeqCst)
    }
    
    // Get receiver count - fast access
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    // Get saved chunks count for diagnostics - fast access
    pub fn get_saved_chunks_count(&self) -> usize {
        let inner = self.inner.lock();
        inner.saved_chunks.len()
    }
}
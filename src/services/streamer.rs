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

// Size of chunks for broadcasting
const BROADCAST_CHUNK_SIZE: usize = 8192; // 8KB chunks for broadcasting

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
    // Broadcast channel for streaming audio to multiple clients
    broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
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
    
    // Active listener count
    active_listeners: usize,
    
    // Stream state
    streaming: bool,
    stream_thread: Option<JoinHandle<()>>,
    
    // Last buffer update time - used for detecting stalled streams
    last_buffer_update: Instant,
    
    // End of track marker
    track_ended: bool,
    
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
}

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, cache_time: u64) -> Self {
        info!("Initializing StreamManager with music_folder: {}, chunk_size: {}, buffer_size: {}, cache_time: {}",
            music_folder.display(), chunk_size, buffer_size, cache_time);
        
        // Create broadcast channel with capacity for 100 messages
        // This allows late joiners to still receive recent data
        let (broadcast_tx, _) = broadcast::channel(100);
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            chunk_size,
            buffer_size,
            cache_time,
            current_track_path: None,
            current_track_info: None,
            buffer: VecDeque::with_capacity(buffer_size / chunk_size),
            chunk_times: VecDeque::with_capacity(buffer_size / chunk_size),
            active_listeners: 0,
            streaming: false,
            stream_thread: None,
            last_buffer_update: Instant::now(),
            track_ended: false,
            playback_position: 0,
            id3_header: None,
            broadcast_tx: broadcast_tx.clone(),
            track_start_time: Instant::now(),
            real_time_position: true, // Use real-time tracking by default
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
            broadcast_tx: Arc::new(broadcast_tx),
        }
    }
    
    pub fn start_streaming(&self, track_path: &str) {
        let mut inner = self.inner.lock();
        
        // If already streaming this track, do nothing
        if inner.current_track_path.as_deref() == Some(track_path) && inner.streaming && !inner.track_ended {
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
        
        // Start new stream thread
        inner.streaming = true;
        inner.track_ended = false;
        inner.last_buffer_update = Instant::now();
        
        // Reset the track start time for real-time position tracking
        inner.track_start_time = Instant::now();
        
        let music_folder = inner.music_folder.clone();
        let track_path = track_path.to_string();
        let chunk_size = BROADCAST_CHUNK_SIZE; // Use broadcast chunk size for streaming
        let inner_clone = self.inner.clone();
        
        debug!("Creating new stream thread for track: {}", track_path);
        let thread_handle = thread::spawn(move || {
            // Stream the track to all listeners
            Self::buffer_track(inner_clone, &music_folder, &track_path, chunk_size);
        });
        
        inner.stream_thread = Some(thread_handle);
    }
    
    // Helper method to clean up existing stream thread
    fn cleanup_stream_thread(&self, inner: &mut StreamManagerInner) {
        // Stop existing stream thread if any
        if inner.streaming && inner.stream_thread.is_some() {
            info!("Stopping existing stream thread");
            inner.streaming = false;
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
                // If thread didn't terminate, we'll just abandon it
                // It will notice streaming is false and exit eventually
            }
        }
    }
    
    fn buffer_track(
        inner: Arc<Mutex<StreamManagerInner>>, 
        music_folder: &Path, 
        track_path: &str, 
        chunk_size: usize
    ) {
        let file_path = music_folder.join(track_path);
        let start_time = std::time::Instant::now();
        
        println!("Starting to buffer track: {}", file_path.display());
        
        if !file_path.exists() {
            println!("ERROR: File not found: {}", file_path.display());
            let mut inner = inner.lock();
            inner.streaming = false;
            inner.track_ended = true;
            return;
        }
        
        println!("Opening file for streaming: {}", file_path.display());
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                println!("ERROR: Error opening file {}: {}", file_path.display(), e);
                let mut inner = inner.lock();
                inner.streaming = false;
                inner.track_ended = true;
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
        
        // Send track info first
        if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE,
            &crate::config::MUSIC_FOLDER
        ) {
            if let Ok(track_info) = serde_json::to_string(&track) {
                println!("Broadcasting track info: {}", track_info);
                let mut inner = inner.lock();
                inner.current_track_info = Some(track_info.clone());
                
                // Broadcast track info to clients
                let _ = inner.broadcast_tx.send(track_info.into_bytes());
            }
        }
        
        // Extract and store the ID3 header (first few KB of MP3 file)
        let mut id3_buffer = vec![0; 16384]; // 16KB should be enough for most ID3 headers
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                // Store ID3 header
                let mut inner = inner.lock();
                inner.id3_header = Some(id3_data.clone());
                
                // Broadcast ID3 header to all listeners
                let _ = inner.broadcast_tx.send(id3_data);
                
                // Reset file position to beginning
                if let Err(e) = file.seek(SeekFrom::Start(0)) {
                    println!("ERROR: Failed to seek back to beginning of file: {}", e);
                    inner.streaming = false;
                    inner.track_ended = true;
                    return;
                }
            },
            Ok(0) => {
                println!("WARNING: Empty file: {}", file_path.display());
                let mut inner = inner.lock();
                inner.streaming = false;
                inner.track_ended = true;
                return;
            },
            Err(e) => {
                println!("ERROR: Failed to read ID3 header: {}", e);
                let mut inner = inner.lock();
                inner.streaming = false;
                inner.track_ended = true;
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
        
        loop {
            // Check if we should continue streaming
            let should_continue = {
                let inner = inner.lock();
                inner.streaming && !eof_reached
            };
            
            if !should_continue {
                println!("Stopping stream thread as requested after {} seconds", start_time.elapsed().as_secs());
                break;
            }
            
            // Update playback position based on real elapsed time (every second)
            if last_position_update.elapsed().as_secs() >= 1 {
                // Get the current playback position based on real elapsed time
                let elapsed_secs = real_start_time.elapsed().as_secs();
                
                {
                    let mut inner_lock = inner.lock();
                    
                    // For internal tracking, always update the byte-based position
                    let byte_based_position = if bytes_per_second > 0 {
                        total_bytes_read / bytes_per_second
                    } else {
                        0
                    };
                    inner_lock.playback_position = byte_based_position;
                    
                    // Don't allow real-time position to exceed track duration
                    if inner_lock.real_time_position && expected_duration > 0 && elapsed_secs > expected_duration {
                        // If we've reached the end of the track duration, don't report position beyond track length
                        // This is handled in get_playback_position
                    }
                }
                
                last_position_update = Instant::now();
            }
            
            // Log progress every 5 seconds
            if last_progress_log.elapsed().as_secs() >= 5 {
                let current_position = {
                    let inner_lock = inner.lock();
                    // Report real or file-based position depending on setting
                    if inner_lock.real_time_position {
                        let elapsed = real_start_time.elapsed().as_secs();
                        // Cap at track duration if needed
                        if expected_duration > 0 && elapsed > expected_duration {
                            expected_duration
                        } else {
                            elapsed
                        }
                    } else {
                        inner_lock.playback_position
                    }
                };
                
                println!("BUFFER STATUS: Broadcasting \"{}\" - {} bytes read ({:.2}% of file) over {} seconds, position={}s", 
                       track_path, 
                       total_bytes_read,
                       if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 },
                       start_time.elapsed().as_secs(),
                       current_position);
                
                // Get receiver count and buffer status
                let (buffer_len, buffer_capacity, receiver_count) = {
                    let inner_lock = inner.lock();
                    (inner_lock.buffer.len(), 
                     inner_lock.buffer.capacity(), 
                     inner_lock.broadcast_tx.receiver_count())
                };
                
                println!("Buffer status: {}/{} chunks ({:.2}%), {} active listeners", 
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
                    
                    // Flag that we'll be waiting
                    let mut wait_active = true;
                    
                    // Track real start of waiting time
                    let wait_start = Instant::now();
                    
                    // Set up a thread to report status during the wait
                    let inner_for_updates = inner.clone();
                    let track_path_for_updates = track_path.to_string();
                    let expected_duration_copy = expected_duration;
                    
                    // If we need to wait more than 10 seconds, spawn a thread to report progress
                    if expected_duration > 0 && position < expected_duration && expected_duration - position > 10 {
                        let wait_active_clone = Arc::new(Mutex::new(wait_active));
                        let wait_active_for_thread = Arc::clone(&wait_active_clone);
                        
                        let update_thread = thread::spawn(move || {
                            let mut last_update = Instant::now();
                            
                            while *wait_active_for_thread.lock() {
                                if last_update.elapsed().as_secs() >= 30 {
                                    // Report current real-time position
                                    let elapsed_secs = real_start_time.elapsed().as_secs();
                                    let current_pos = if expected_duration_copy > 0 && elapsed_secs > expected_duration_copy {
                                        expected_duration_copy
                                    } else {
                                        elapsed_secs
                                    };
                                    
                                    println!("Still waiting for \"{}\" to complete... Position: {}s of {}s", 
                                           track_path_for_updates, current_pos, expected_duration_copy);
                                    
                                    last_update = Instant::now();
                                }
                                
                                thread::sleep(Duration::from_secs(1));
                            }
                        });
                        
                        // Immediately detach the thread
                        drop(update_thread);
                    }
                    
                    if expected_duration > 0 && position < expected_duration {
                        let wait_seconds = expected_duration - position;
                        println!("Waiting {} more seconds to complete full track duration", wait_seconds);
                        
                        // Use a much longer sleep interval to reduce log spam and improve efficiency
                        let sleep_interval = 10; // 10 seconds
                        let mut remaining = wait_seconds;
                        
                        while remaining > 0 {
                            // Check if we should continue or were asked to stop
                            let should_continue = {
                                let inner = inner.lock();
                                inner.streaming && !inner.track_ended
                            };
                            
                            if !should_continue {
                                println!("Stopping wait due to external request");
                                break;
                            }
                            
                            // Sleep for the interval or the remaining time, whichever is smaller
                            let sleep_time = std::cmp::min(sleep_interval, remaining);
                            thread::sleep(Duration::from_secs(sleep_time));
                            remaining -= sleep_time;
                            
                            // Update position based on elapsed time since we started waiting
                            // This is more accurate than incrementing by sleep_time
                            let elapsed_wait = wait_start.elapsed().as_secs();
                            let new_position = position + std::cmp::min(elapsed_wait, wait_seconds);
                            
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
                    
                    // Signal that we're done waiting
                    wait_active = false;
                    
                    println!("Track playback complete after waiting: {} actual seconds", start_time.elapsed().as_secs());
                    
                    // Signal the end of track
                    {
                        let mut inner_lock = inner.lock();
                        inner_lock.track_ended = true;
                        
                        // Send empty chunk to signal end of track to clients
                        let _ = inner_lock.broadcast_tx.send(Vec::new());
                        
                        // Wait a bit then set streaming to false to allow final chunk delivery
                        thread::sleep(Duration::from_millis(500));
                        inner_lock.streaming = false;
                    }
                    
                    println!("Set track_ended flag as track has completed after {} seconds", start_time.elapsed().as_secs());
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
                    
                    // Add to buffer and update stream state
                    {
                        let mut inner = inner.lock();
                        
                        // Add to buffer (for recovery/late joiners)
                        inner.buffer.push_back(chunk_data.clone());
                        inner.chunk_times.push_back(Instant::now());
                        inner.last_buffer_update = Instant::now();
                        
                        // Trim buffer if it gets too large
                        while inner.buffer.len() > inner.buffer.capacity() {
                            inner.buffer.pop_front();
                            inner.chunk_times.pop_front();
                        }
                        
                        // Broadcast the chunk to all listeners
                        let _ = inner.broadcast_tx.send(chunk_data);
                    }
                    
                    // Sleep briefly to control broadcast rate
                    // This helps prevent buffer overruns in clients
                    thread::sleep(Duration::from_millis(5));
                },
                Err(e) => {
                    println!("ERROR: Error reading file {}: {}", file_path.display(), e);
                    
                    // Set streaming state to error
                    let mut inner = inner.lock();
                    inner.streaming = false;
                    inner.track_ended = true;
                    
                    // Send end of track signal
                    let _ = inner.broadcast_tx.send(Vec::new());
                    
                    break;
                }
            }
        }
    }

    pub fn force_stop_streaming(&self) {
        let mut inner = self.inner.lock();
        inner.streaming = false;
        inner.track_ended = true;
        println!("Force stopped broadcasting by setting streaming and track_ended flags");
    }
    
    // Get a broadcast receiver for clients to listen to the stream
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.broadcast_tx.subscribe()
    }
    
    // Get ID3 header for new connections
    pub fn get_id3_header(&self) -> Option<Vec<u8>> {
        let inner = self.inner.lock();
        inner.id3_header.clone()
    }
    
    // Get current track info
    pub fn get_track_info(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.current_track_info.clone()
    }
    
    pub fn get_active_listeners(&self) -> usize {
        let inner = self.inner.lock();
        inner.active_listeners
    }
    
    pub fn is_streaming(&self) -> bool {
        let inner = self.inner.lock();
        inner.streaming
    }
    
    pub fn track_ended(&self) -> bool {
        let inner = self.inner.lock();
        inner.track_ended
    }
    
    pub fn increment_listener_count(&self) {
        let mut inner = self.inner.lock();
        inner.active_listeners += 1;
        info!("Listener connected. Active listeners: {}", inner.active_listeners);
    }

    pub fn decrement_listener_count(&self) {
        let mut inner = self.inner.lock();
        if inner.active_listeners > 0 {
            inner.active_listeners -= 1;
        }
        info!("Listener disconnected. Active listeners: {}", inner.active_listeners);
    }
    
    pub fn inner(&self) -> &Self {
        self
    }
    
    pub fn get_current_track_path(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.current_track_path.clone()
    }
    
    // Modified get_playback_position method
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
    
    // Toggle between real-time and file-based position tracking
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
    
    pub fn reset_track_ended_flag(&self) {
        let mut inner = self.inner.lock();
        inner.track_ended = false;
    }
    
    // Check if the stream is stalled (no buffer updates for a long time)
    pub fn is_stream_stalled(&self) -> bool {
        let inner = self.inner.lock();
        inner.last_buffer_update.elapsed() > Duration::from_secs(10) && inner.streaming
    }
    
    // Get number of current broadcast receivers
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
}
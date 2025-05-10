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
use std::sync::atomic::{AtomicBool, AtomicUsize, AtomicU64, Ordering};
use id3::TagLike;

// Global thread counter for debugging
static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);

// Size of chunks for broadcasting
const BROADCAST_CHUNK_SIZE: usize = 16384; // Increase from 8KB to 16KB for better buffering
const BROADCAST_RATE_LIMITER_MS: u64 = 10; // Reduce from 50ms to 10ms minimum
const MIN_BUFFER_BEFORE_PLAY: usize = 10; // Ensure at least 10 chunks are buffered

// Constants for live broadcast joining
const LIVE_JOIN_BUFFER_SECONDS: u64 = 3; // How many seconds of recent audio to send to new clients
const MAX_RECENT_CHUNKS_FOR_LIVE: usize = 50; // Maximum recent chunks to keep for live joining (~6 seconds at 128kbps)
const LIVE_JOIN_ENABLED: bool = true; // Whether to join live broadcast or start from beginning

// Add these new constants for adaptive streaming
const ADAPTIVE_RATE_ADJUSTMENT: bool = true;
const BITRATE_MARGIN: f64 = 1.2; // 20% margin for bitrate variations

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
            saved_chunks: VecDeque::with_capacity(MAX_RECENT_CHUNKS_FOR_LIVE),
            max_saved_chunks: MAX_RECENT_CHUNKS_FOR_LIVE,
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
        // First, check if we're already streaming this track
        let needs_transition = {
            let inner = self.inner.lock();
            inner.current_track_path.as_deref() != Some(track_path) || 
            self.track_ended.load(Ordering::SeqCst)
        };
        
        if !needs_transition {
            println!("Already streaming track: {}", track_path);
            return;
        }
        
        // Signal track transition to clients with a clear separator
        if self.is_streaming.load(Ordering::SeqCst) {
            // Send track transition marker
            let transition_marker = vec![0xFF, 0xFE]; // Special marker for track change
            let _ = self.broadcast_tx.send(transition_marker);
            
            // Send a few empty chunks to ensure buffer is flushed
            for _ in 0..5 {
                let _ = self.broadcast_tx.send(vec![]);
            }
            
            // Brief pause for clients to process
            thread::sleep(Duration::from_millis(200));
        }
        
        // Now reset all flags properly
        self.is_streaming.store(true, Ordering::SeqCst);
        self.track_ended.store(false, Ordering::SeqCst);
        
        println!("Start streaming - reset streaming flags to correct state");
        
        // Now acquire the mutex for the actual work
        let mut inner = self.inner.lock();
        
        info!("Starting to stream track: {}", track_path);
        
        // Clean up existing stream thread if any
        self.cleanup_stream_thread(&mut inner);
        
        // CRITICAL: Clear ALL buffers for the new track - ensure clean start
        inner.buffer.clear();
        inner.chunk_times.clear();
        inner.playback_position = 0;
        inner.id3_header = None;
        
        // CRITICAL: Clear saved chunks completely
        inner.saved_chunks.clear();
        
        // Force drop and recreate the saved_chunks to ensure it's completely empty
        inner.saved_chunks = VecDeque::with_capacity(MAX_RECENT_CHUNKS_FOR_LIVE);
        
        debug!("Cleared all buffers for new track - ensured no audio mixing");
        
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
        let chunk_size = BROADCAST_CHUNK_SIZE;
        let inner_clone = self.inner.clone();
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        
        // Generate unique thread ID using atomic counter
        let thread_id = THREAD_COUNTER.fetch_add(1, Ordering::SeqCst);
        
        println!("Creating new stream thread {} for track: {}", thread_id, track_path);
        
        // Create new thread for streaming
        let track_path_clone = track_path.clone();
        let thread_handle = thread::spawn(move || {
            println!("Stream thread {} started for: {}", thread_id, track_path_clone);
            Self::buffer_track(thread_id, inner_clone, broadcast_tx, is_streaming, track_ended, &music_folder, &track_path_clone, chunk_size);
            println!("Stream thread {} finished for: {}", thread_id, track_path_clone);
        });
        
        inner.stream_thread = Some(thread_handle);
    }
    
    fn cleanup_stream_thread(&self, inner: &mut StreamManagerInner) {
        // Store thread locally, drop lock completely when joining
        if let Some(thread) = inner.stream_thread.take() {
            info!("Stopping existing stream thread");
            
            // Signal the thread to stop by setting a flag
            self.track_ended.store(true, Ordering::SeqCst);
            
            // Drop the inner lock completely before operations that might block
            std::mem::drop(inner);
            
            // Force the thread to wake up if it's sleeping
            let _ = self.broadcast_tx.send(vec![0xFF, 0xFD]); // Stop marker
            
            // Brief pause to allow thread to react
            thread::sleep(Duration::from_millis(100));
            
            // Try to join with a simple approach - just join and handle the result
            info!("Attempting to join stream thread");
            match thread.join() {
                Ok(_) => {
                    info!("Stream thread joined successfully");
                }
                Err(e) => {
                    error!("Error joining stream thread: {:?}", e);
                }
            }
        }
    }
    
    fn buffer_track(
        thread_id: u64,
        inner: Arc<Mutex<StreamManagerInner>>, 
        broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        music_folder: &Path, 
        track_path: &str, 
        chunk_size: usize
    ) {
        let file_path = music_folder.join(track_path);
        let start_time = std::time::Instant::now();
        
        println!("Thread {}: Buffer track starting for: {}", thread_id, file_path.display());
        
        // Check if we should even start - might have been signaled to stop
        if track_ended.load(Ordering::SeqCst) {
            println!("Thread {}: Track already ended, not starting buffer for: {}", thread_id, file_path.display());
            return;
        }
        
        if !file_path.exists() {
            println!("Thread {}: ERROR: File not found: {}", thread_id, file_path.display());
            track_ended.store(true, Ordering::SeqCst);
            return;
        }
        
        println!("Thread {}: Opening file for streaming: {}", thread_id, file_path.display());
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                println!("Thread {}: ERROR: Error opening file {}: {}", thread_id, file_path.display(), e);
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
        
        println!("Thread {}: Starting to broadcast file: {}, size: {} bytes, expected duration: {} seconds", 
                 thread_id, file_path.display(), file_size, expected_duration);
        
        // Improve the bitrate calculation
        let mut actual_bitrate = 0u64;
        
        // Try to get actual bitrate from MP3 metadata
        if let Ok(tag) = id3::Tag::read_from_path(&file_path) {
            if let Some(frame) = tag.get("TLEN") {
                if let Ok(duration_ms) = frame.content().text().unwrap_or("0").parse::<u64>() {
                    if duration_ms > 0 && file_size > 0 {
                        actual_bitrate = (file_size * 8 * 1000) / duration_ms;
                        println!("Thread {}: Detected actual bitrate from ID3: {} bps", thread_id, actual_bitrate);
                    }
                }
            }
        }
        
        // If we couldn't get bitrate from metadata, use estimation
        if actual_bitrate == 0 {
            // Read first few MP3 frames to estimate bitrate
            let mut sample_buffer = vec![0u8; 32768]; // 32KB sample
            if let Ok(bytes_read) = file.read(&mut sample_buffer) {
                actual_bitrate = estimate_mp3_bitrate(&sample_buffer[..bytes_read]);
                file.seek(SeekFrom::Start(0)).ok();
            }
        }
        
        // Fall back to file size / duration if needed
        if actual_bitrate == 0 && file_size > 0 && expected_duration > 0 {
            actual_bitrate = (file_size * 8) / expected_duration;
        }
        
        // Default to 128kbps if all else fails
        if actual_bitrate == 0 {
            actual_bitrate = 128000;
        }
        
        println!("Thread {}: Using bitrate: {} bps for streaming calculations", thread_id, actual_bitrate);
        
        // Calculate chunk delay with margin for variations
        let bytes_per_second = actual_bitrate / 8;
        let adjusted_bytes_per_second = if ADAPTIVE_RATE_ADJUSTMENT {
            (bytes_per_second as f64 * BITRATE_MARGIN) as u64
        } else {
            bytes_per_second
        };
        
        let bytes_per_chunk = chunk_size as f64;
        let chunks_per_second = adjusted_bytes_per_second as f64 / bytes_per_chunk;
        let chunk_delay_ms = 1000.0 / chunks_per_second;
        
        // Use adaptive delay with minimum threshold
        let chunk_delay = if chunk_delay_ms < BROADCAST_RATE_LIMITER_MS as f64 {
            Duration::from_millis(BROADCAST_RATE_LIMITER_MS)
        } else {
            Duration::from_millis(chunk_delay_ms as u64)
        };
        
        println!("Thread {}: Adaptive streaming: chunk delay = {:.2}ms for bitrate {}kbps", 
                 thread_id, chunk_delay.as_millis(), actual_bitrate / 1000);
        
        // Send track info first
        if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE,
            &crate::config::MUSIC_FOLDER
        ) {
            if let Ok(track_info) = serde_json::to_string(&track) {
                println!("Thread {}: Broadcasting track info: {}", thread_id, track_info);
                
                // Update track info with minimal locking
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.current_track_info = Some(track_info.clone());
                    let _ = inner_lock.broadcast_tx.send(track_info.into_bytes());
                } else {
                    // If we can't get lock, just broadcast directly
                    let _ = broadcast_tx.send(track_info.into_bytes());
                }
            }
        }
        
        // Extract and store the ID3 header (first few KB of MP3 file)
        let mut id3_buffer = vec![0; 16384]; // 16KB should be enough for most ID3 headers
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                // Store ID3 header with minimal locking
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.id3_header = Some(id3_data.clone());
                    let _ = inner_lock.broadcast_tx.send(id3_data);
                } else {
                    // If we can't get lock, just broadcast directly
                    let _ = broadcast_tx.send(id3_data);
                }
                
                // Reset file position to beginning
                if let Err(e) = file.seek(SeekFrom::Start(0)) {
                    println!("Thread {}: ERROR: Failed to seek back to beginning of file: {}", thread_id, e);
                    track_ended.store(true, Ordering::SeqCst);
                    return;
                }
            },
            Ok(0) => {
                println!("Thread {}: WARNING: Empty file: {}", thread_id, file_path.display());
                track_ended.store(true, Ordering::SeqCst);
                return;
            },
            Err(e) => {
                println!("Thread {}: ERROR: Failed to read ID3 header: {}", thread_id, e);
                track_ended.store(true, Ordering::SeqCst);
                return;
            },
            _ => {} // Other cases handled by compiler
        }
        
        // Continue with normal buffering
        let mut buffer = vec![0; chunk_size + 4]; // Add a small margin for MP3 frame alignment
        let mut last_progress_log = std::time::Instant::now();
        let mut total_bytes_read = 0;
        let mut chunks_sent = 0;
        
        // Track real elapsed time since starting
        let real_start_time = Instant::now();
        
        // Track playback position
        let mut last_position_update = Instant::now();
        
        // To store any leftover bytes between chunks
        let mut leftover_bytes: Vec<u8> = Vec::new();
        
        let mut eof_reached = false;
        
        while is_streaming.load(Ordering::SeqCst) && !eof_reached {
            // Check if this thread should stop (track has ended)
            if track_ended.load(Ordering::SeqCst) {
                println!("Thread {}: Track ended flag detected, stopping buffer thread for: {}", thread_id, track_path);
                break;
            }
            
            // Update playback position based on real elapsed time (every second)
            if last_position_update.elapsed().as_secs() >= 1 {
                // Calculate position based on bytes read and expected bitrate
                let elapsed_secs = real_start_time.elapsed().as_secs();
                let byte_based_position = if bytes_per_second > 0 {
                    total_bytes_read / bytes_per_second
                } else {
                    elapsed_secs
                };
                
                // Update position with brief locking
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.playback_position = byte_based_position;
                }
                
                last_position_update = Instant::now();
            }
            
            // Log progress every 5 seconds
            if last_progress_log.elapsed().as_secs() >= 5 {
                // Get position with minimal locking
                if let Some(inner_lock) = inner.try_lock() {
                    let current_position = if inner_lock.real_time_position {
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
                    
                    println!("Thread {}: BUFFER STATUS: Broadcasting \"{}\" - {} bytes read ({:.2}% of file) over {} seconds, position={}s", 
                           thread_id,
                           track_path, 
                           total_bytes_read,
                           if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 },
                           start_time.elapsed().as_secs(),
                           current_position);
                    
                    println!("Thread {}: Buffer status: {}/{} chunks ({:.2}%), {} active receivers", 
                           thread_id,
                           inner_lock.buffer.len(), inner_lock.buffer.capacity(), 
                           if inner_lock.buffer.capacity() > 0 { 
                               (inner_lock.buffer.len() as f64 / inner_lock.buffer.capacity() as f64) * 100.0
                           } else { 0.0 },
                           inner_lock.broadcast_tx.receiver_count());
                    
                    // Also log saved chunks status for live join
                    println!("Thread {}: Saved chunks for live join: {} (max: {})", 
                           thread_id, inner_lock.saved_chunks.len(), MAX_RECENT_CHUNKS_FOR_LIVE);
                }
                
                last_progress_log = std::time::Instant::now();
            }
            
            // Prepare buffer that includes any leftover bytes
            let mut read_buffer = vec![0; chunk_size];
            
            // Read the next chunk
            match file.read(&mut read_buffer) {
                Ok(0) => {
                    // End of file reached
                    eof_reached = true;
                    
                    // Send any remaining leftover bytes as the final chunk
                    if !leftover_bytes.is_empty() {
                        if let Some(mut inner_lock) = inner.try_lock() {
                            inner_lock.buffer.push_back(leftover_bytes.clone());
                            inner_lock.chunk_times.push_back(Instant::now());
                            inner_lock.last_buffer_update = Instant::now();
                            
                            // Add to saved chunks for new clients
                            inner_lock.saved_chunks.push_back(leftover_bytes.clone());
                            
                            // Keep saved chunks within size limit
                            while inner_lock.saved_chunks.len() > MAX_RECENT_CHUNKS_FOR_LIVE {
                                inner_lock.saved_chunks.pop_front();
                            }
                            
                            // Broadcast final chunk
                            let _ = inner_lock.broadcast_tx.send(leftover_bytes.clone());
                        } else {
                            // If we can't get lock, just broadcast directly
                            let _ = broadcast_tx.send(leftover_bytes);
                        }
                        leftover_bytes = Vec::new();
                    }
                    
                    // Wait for at least expected_duration before ending the track
                    // This prevents premature track ending due to fast reading
                    let elapsed = start_time.elapsed().as_secs();
                    let position = if bytes_per_second > 0 {
                        total_bytes_read / bytes_per_second
                    } else {
                        elapsed
                    };
                    
                    println!("Thread {}: End of file reached for track: {} after {} seconds, position={}s of expected {}s", 
                           thread_id, track_path, elapsed, position, expected_duration);
                    
                    // If we need to wait to reach expected duration
                    if expected_duration > 0 && position < expected_duration {
                        let wait_seconds = expected_duration - position;
                        println!("Thread {}: Waiting {} more seconds to complete full track duration", thread_id, wait_seconds);
                        
                        // Use a longer sleep interval for better efficiency
                        let sleep_interval = 1; // 1 second
                        let mut remaining = wait_seconds;
                        
                        while remaining > 0 && is_streaming.load(Ordering::SeqCst) {
                            // Check if we should stop during waiting
                            if track_ended.load(Ordering::SeqCst) {
                                println!("Thread {}: Track ended during duration wait", thread_id);
                                break;
                            }
                            
                            // Sleep for the interval or the remaining time, whichever is smaller
                            let sleep_time = std::cmp::min(sleep_interval, remaining);
                            thread::sleep(Duration::from_secs(sleep_time));
                            remaining -= sleep_time;
                            
                            // Update position based on elapsed time
                            let new_position = position + (wait_seconds - remaining);
                            
                            // Update with minimal locking
                            if let Some(mut inner_lock) = inner.try_lock() {
                                inner_lock.playback_position = new_position;
                            }
                            
                            // Log progress every 30 seconds or at the end
                            if remaining % 30 == 0 || remaining < sleep_interval {
                                println!("Thread {}: Track \"{}\" at position {}s of {}s, {} seconds remaining", 
                                       thread_id, track_path, new_position, expected_duration, 
                                       if new_position < expected_duration { expected_duration - new_position } else { 0 });
                            }
                        }
                    }
                    
                    println!("Thread {}: Track playback complete after waiting: {} actual seconds", thread_id, start_time.elapsed().as_secs());
                    
                    // Send proper track end marker
                    let end_marker = vec![0xFF, 0xFF]; // Track end marker
                    let _ = broadcast_tx.send(end_marker);
                    
                    // Signal the end of track using atomic flag
                    track_ended.store(true, Ordering::SeqCst);
                    
                    println!("Thread {}: Track \"{}\" ended properly - STREAMING REMAINS ACTIVE", thread_id, track_path);
                    break;
                },
                Ok(n) => {
                    // Got a chunk of data
                    let mut current_data: Vec<u8> = Vec::new();
                    
                    // Combine leftover bytes with new data
                    if !leftover_bytes.is_empty() {
                        current_data.extend_from_slice(&leftover_bytes);
                    }
                    current_data.extend_from_slice(&read_buffer[..n]);
                    
                    // Find MP3 frame boundaries to ensure we send clean chunks
                    // An MP3 frame starts with a sync word: 0xFF followed by bits 111xxxxx (0xE0-0xFF)
                    let mut start_index = 0;
                    
                    // Find the first frame start if we're at the beginning
                    if chunks_sent == 0 && leftover_bytes.is_empty() {
                        // Look for the first MP3 frame start
                        for i in 0..current_data.len().saturating_sub(1) {
                            if current_data[i] == 0xFF && (current_data[i+1] & 0xE0) == 0xE0 {
                                start_index = i;
                                break;
                            }
                        }
                    }
                    
                    // Find where the last complete frame ends
                    let mut end_index = current_data.len();
                    
                    // Look backwards from the end to find the last complete frame
                    // We need at least 4 bytes for a minimal MP3 frame
                    if current_data.len() > start_index + 4 {
                        for i in (start_index..current_data.len() - 4).rev() {
                            if current_data[i] == 0xFF && (current_data[i+1] & 0xE0) == 0xE0 {
                                // Check if this frame can be complete within our buffer
                                // A very simple check - just ensure we have at least a few bytes
                                let min_frame_size = 48; // Minimal MP3 frame size for safety
                                if i + min_frame_size <= current_data.len() {
                                    // We found a likely complete frame
                                    // Set the end after this frame
                                    end_index = i + min_frame_size;
                                    break;
                                }
                            }
                        }
                    }
                    
                    // Check if we have a valid chunk to send
                    if end_index > start_index {
                        // Extract the chunk to send
                        let chunk_data = current_data[start_index..end_index].to_vec();
                        
                        // Save the remainder as leftover bytes
                        leftover_bytes = if end_index < current_data.len() {
                            current_data[end_index..].to_vec()
                        } else {
                            Vec::new()
                        };
                        
                        chunks_sent += 1;
                        total_bytes_read += chunk_data.len() as u64;
                        
                        if chunks_sent % 100 == 0 {
                            println!("Thread {}: Sent {} chunks, {} bytes ({:.2}% of file)", 
                                   thread_id, chunks_sent, total_bytes_read,
                                   if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 });
                        }
                        
                        // NON-BLOCKING BROADCAST: Try to update buffers and broadcast
                        match inner.try_lock() {
                            Some(mut inner_lock) => {
                                // We got the lock - update buffers as normal
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
                                
                                // Keep saved chunks limited to recent history for live join
                                // This ensures new clients join at current position
                                while inner_lock.saved_chunks.len() > MAX_RECENT_CHUNKS_FOR_LIVE {
                                    inner_lock.saved_chunks.pop_front();
                                }
                                
                                // Broadcast the chunk to all listeners
                                let _ = inner_lock.broadcast_tx.send(chunk_data);
                            },
                            None => {
                                // Failed to get lock - prioritize real-time delivery
                                // Just broadcast directly without updating buffers
                                let _ = broadcast_tx.send(chunk_data);
                                
                                // Log occasionally to avoid spam
                                if chunks_sent % 20 == 10 {
                                    debug!("Thread {}: Broadcasted chunk directly without buffer update (lock contention)", thread_id);
                                }
                            }
                        }
                    } else {
                        // If we can't find good MP3 frames, just store all as leftover
                        leftover_bytes = current_data;
                    }
                    
                    // Sleep to control broadcast rate
                    thread::sleep(chunk_delay);
                },
                Err(e) => {
                    println!("Thread {}: ERROR: Error reading file {}: {}", thread_id, file_path.display(), e);
                    
                    // Set error state with minimal locking - but keep streaming active
                    track_ended.store(true, Ordering::SeqCst);
                    
                    // Send end of track signal
                    let _ = broadcast_tx.send(Vec::new());
                    
                    break;
                }
            }
        }
        
        println!("Thread {}: Exiting buffer_track thread for track: {}", thread_id, track_path);
    }
    
    // Non-blocking method to get saved chunks - returns immediately without waiting
    pub fn get_saved_chunks(&self) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        let guard = self.inner.lock();
        
        // Return ID3 header and saved chunks (cloned to avoid holding the lock)
        let header = guard.id3_header.clone();
        let chunks: Vec<Vec<u8>> = guard.saved_chunks.iter().cloned().collect();
        
        (header, chunks)
    }
    
    // Method to get saved chunks starting from current playback position for live join
    pub fn get_chunks_from_current_position(&self) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        let guard = self.inner.lock();
        
        // Get ID3 header
        let header = guard.id3_header.clone();
        
        if LIVE_JOIN_ENABLED {
            // For live streaming, we want to give new clients only the most recent chunks
            // This ensures they join the broadcast at the current position, not from the beginning
            
            // Calculate how many chunks to keep for new clients
            let bytes_per_second = 16000; // Approximate 128kbps
            let bytes_to_keep = LIVE_JOIN_BUFFER_SECONDS * bytes_per_second;
            let chunks_to_keep = (bytes_to_keep / BROADCAST_CHUNK_SIZE as u64) as usize;
            
            // Get only the most recent chunks
            let saved_chunks: Vec<Vec<u8>> = if guard.saved_chunks.len() > chunks_to_keep {
                // Skip older chunks and return only recent ones
                let skip_count = guard.saved_chunks.len() - chunks_to_keep;
                guard.saved_chunks.iter().skip(skip_count).cloned().collect()
            } else {
                // If we have fewer chunks than the threshold, return all
                guard.saved_chunks.iter().cloned().collect()
            };
            
            println!("New client joining live stream. Sending {} recent chunks (last {}s) out of {} total", 
                     saved_chunks.len(), LIVE_JOIN_BUFFER_SECONDS, guard.saved_chunks.len());
            
            (header, saved_chunks)
        } else {
            // If live join is disabled, return all saved chunks (old behavior)
            let saved_chunks: Vec<Vec<u8>> = guard.saved_chunks.iter().cloned().collect();
            println!("New client starting from beginning. Sending all {} saved chunks", saved_chunks.len());
            (header, saved_chunks)
        }
    }
    
    // Add a method to force next track
    pub fn force_next_track(&self) {
        println!("Forcing switch to next track due to timeout");
        
        // Signal end of track to clients
        let _ = self.broadcast_tx.send(Vec::new());
        
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
        let _ = self.broadcast_tx.send(Vec::new());
        
        println!("Force stopped broadcasting by setting streaming and track_ended flags");
    }
    
    // Get a broadcast receiver for clients to listen to the stream
    pub fn get_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.broadcast_tx.subscribe()
    }
    
    // Get ID3 header for new connections - non-blocking
    pub fn get_id3_header(&self) -> Option<Vec<u8>> {
        if let Some(inner) = self.inner.try_lock() {
            inner.id3_header.clone()
        } else {
            None
        }
    }
    
    // Get current track info - non-blocking
    pub fn get_track_info(&self) -> Option<String> {
        if let Some(inner) = self.inner.try_lock() {
            inner.current_track_info.clone()
        } else {
            None
        }
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
        if let Some(inner) = self.inner.try_lock() {
            inner.current_track_path.clone()
        } else {
            None
        }
    }
    
    // Get playback position - quick access
    pub fn get_playback_position(&self) -> u64 {
        if let Some(inner) = self.inner.try_lock() {
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
        } else {
            0 // Return 0 if we can't get lock
        }
    }
    
    pub fn set_real_time_position(&self, enabled: bool) {
        if let Some(mut inner) = self.inner.try_lock() {
            inner.real_time_position = enabled;
            info!("Real-time position tracking {}", if enabled { "enabled" } else { "disabled" });
        }
    }
    
    pub fn is_real_time_position(&self) -> bool {
        if let Some(inner) = self.inner.try_lock() {
            inner.real_time_position
        } else {
            true // Default to true
        }
    }
    
    pub fn buffer_status(&self) -> (usize, usize) {
        if let Some(inner) = self.inner.try_lock() {
            (inner.buffer.len(), inner.buffer.capacity())
        } else {
            (0, 0)
        }
    }
    
    // Use atomic flag for quick updates
    pub fn reset_track_ended_flag(&self) {
        self.track_ended.store(false, Ordering::SeqCst);
    }
    
    // Check for stalled streams
    pub fn is_stream_stalled(&self) -> bool {
        if let Some(inner) = self.inner.try_lock() {
            inner.last_buffer_update.elapsed() > Duration::from_secs(10) && self.is_streaming.load(Ordering::SeqCst)
        } else {
            false
        }
    }
    
    // Get receiver count - fast access
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    // Get saved chunks count for diagnostics - fast access
    pub fn get_saved_chunks_count(&self) -> usize {
        if let Some(inner) = self.inner.try_lock() {
            inner.saved_chunks.len()
        } else {
            0
        }
    }
    
    // Helper function to find an MP3 frame boundary in a buffer
    fn find_mp3_frame_boundary(data: &[u8]) -> Option<usize> {
        // Look for MP3 frame sync (0xFF followed by 0xE0-0xFF)
        for i in 0..data.len().saturating_sub(1) {
            if data[i] == 0xFF && (data[i+1] & 0xE0) == 0xE0 {
                return Some(i);
            }
        }
        None
    }
}

// Add helper function to estimate MP3 bitrate
fn estimate_mp3_bitrate(data: &[u8]) -> u64 {
    // Look for MP3 frame headers and calculate average bitrate
    let mut total_bitrate = 0u64;
    let mut frame_count = 0;
    let mut pos = 0;
    
    while pos < data.len() - 4 {
        if data[pos] == 0xFF && (data[pos + 1] & 0xE0) == 0xE0 {
            // Found potential frame header
            let header = ((data[pos + 1] as u32) << 16) | 
                        ((data[pos + 2] as u32) << 8) | 
                        (data[pos + 3] as u32);
            
            // Extract bitrate index
            let bitrate_index = (header >> 12) & 0x0F;
            let mpeg_version = (header >> 19) & 0x03;
            let layer = (header >> 17) & 0x03;
            
            if let Some(bitrate) = get_mp3_bitrate(mpeg_version, layer, bitrate_index) {
                total_bitrate += bitrate;
                frame_count += 1;
            }
            
            pos += 4; // Move to next potential frame
        } else {
            pos += 1;
        }
    }
    
    if frame_count > 0 {
        (total_bitrate / frame_count) * 1000 // Convert to bps
    } else {
        128000 // Default to 128kbps
    }
}

// Add MP3 bitrate lookup table
fn get_mp3_bitrate(version: u32, layer: u32, bitrate_index: u32) -> Option<u64> {
    // Simplified bitrate table for MPEG1 Layer III (MP3)
    let bitrates = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0
    ];
    
    if version == 3 && layer == 1 && bitrate_index < 16 {
        Some(bitrates[bitrate_index as usize])
    } else {
        None
    }
}
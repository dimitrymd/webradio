// Replace the entire src/services/streamer.rs with this improved single-threaded design:

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
// use id3::TagLike; // Removed unused import

// Constants
const BROADCAST_CHUNK_SIZE: usize = 16384; // 16KB chunks
const BROADCAST_RATE_LIMITER_MS: u64 = 10;
const MAX_RECENT_CHUNKS_FOR_LIVE: usize = 50;

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

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager with single broadcast thread architecture");
        
        let (broadcast_tx, _) = broadcast::channel(200);
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
            saved_chunks: VecDeque::with_capacity(MAX_RECENT_CHUNKS_FOR_LIVE),
            max_saved_chunks: MAX_RECENT_CHUNKS_FOR_LIVE,
            broadcast_thread: None,
            should_stop: should_stop.clone(),
        };
        
        let manager = Self {
            inner: Arc::new(Mutex::new(inner)),
            broadcast_tx: Arc::new(broadcast_tx),
            active_listeners: Arc::new(AtomicUsize::new(0)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            track_ended: Arc::new(AtomicBool::new(false)),
        };
        
        // Don't start the thread here - let the main function do it after ensuring tracks exist
        
        manager
    }

    pub fn get_current_track(&self) -> Option<crate::models::playlist::Track> {
        let inner = self.inner.lock();
        if let Some(track_json) = &inner.current_track_info {
            serde_json::from_str(track_json).ok()
        } else {
            None
        }
    }
    
    pub fn start_broadcast_thread(&self) {
        let mut inner = self.inner.lock();
        
        // Make sure we don't already have a broadcast thread
        if inner.broadcast_thread.is_some() {
            warn!("Broadcast thread already exists");
            return;
        }
        
        let music_folder = inner.music_folder.clone();
        let inner_clone = self.inner.clone();
        let is_streaming = self.is_streaming.clone();
        let track_ended = self.track_ended.clone();
        let should_stop = inner.should_stop.clone();
        
        info!("Starting single broadcast thread");
        
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
        
        let mut last_track_path: Option<String> = None;
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get the current track to play
            let track = match crate::services::playlist::get_current_track(
                &crate::config::PLAYLIST_FILE,
                &crate::config::MUSIC_FOLDER,
            ) {
                Some(track) => track,
                None => {
                    warn!("No tracks available, waiting...");
                    thread::sleep(Duration::from_secs(1));
                    // Try to scan for new tracks
                    crate::services::playlist::scan_music_folder(
                        &crate::config::MUSIC_FOLDER,
                        &crate::config::PLAYLIST_FILE,
                    );
                    continue;
                }
            };
            
            // Check if we're about to play the same track again
            if let Some(ref last_path) = last_track_path {
                if last_path == &track.path {
                    warn!("Detected same track about to play again: {}. Forcing advance.", track.path);
                    // Force advance to next track
                    if let Some(next_track) = crate::services::playlist::advance_track(
                        &crate::config::PLAYLIST_FILE,
                        &crate::config::MUSIC_FOLDER,
                    ) {
                        info!("Advanced to next track: {}", next_track.path);
                        continue; // Go back to the beginning of the loop to play the new track
                    } else {
                        warn!("No next track available, continuing with current");
                    }
                }
            }
            
            let track_path = music_folder.join(&track.path);
            info!("Starting to broadcast track: {} ({})", track.title, track_path.display());
            
            // Update current track info
            {
                let mut inner_lock = inner.lock();
                inner_lock.current_track_path = Some(track.path.clone());
                inner_lock.track_start_time = Instant::now();
                inner_lock.playback_position = 0;
                
                // Prepare track info JSON
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json.clone());
                    
                    // Send track info
                    let _ = inner_lock.broadcast_tx.send(track_json.into_bytes());
                }
                
                // Clear saved chunks for new track
                inner_lock.saved_chunks.clear();
            }
            
            // Remember what track we're playing
            last_track_path = Some(track.path.clone());
            
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
            
            // Track has ended, send transition marker
            if !should_stop.load(Ordering::SeqCst) {
                info!("Track {} finished, preparing transition", track.title);
                
                let transition_marker = vec![0xFF, 0xFE];
                if let Some(mut inner_lock) = inner.try_lock() {
                    let _ = inner_lock.broadcast_tx.send(transition_marker);
                }
                
                // Small delay before moving to next track
                thread::sleep(Duration::from_millis(500));
                
                // Advance to next track BEFORE the loop continues
                match crate::services::playlist::advance_track(
                    &crate::config::PLAYLIST_FILE,
                    &crate::config::MUSIC_FOLDER,
                ) {
                    Some(next_track) => {
                        info!("Advanced playlist to: {} by {}", next_track.title, next_track.artist);
                        // Clear the track ended flag
                        track_ended.store(false, Ordering::SeqCst);
                    },
                    None => {
                        error!("Failed to advance to next track");
                        // Try rescanning
                        crate::services::playlist::scan_music_folder(
                            &crate::config::MUSIC_FOLDER,
                            &crate::config::PLAYLIST_FILE,
                        );
                    }
                }
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
    ) {
        let track_start = Instant::now();
        info!("Broadcasting single track: {} (duration: {}s)", track.title, track.duration);
        
        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(e) => {
                error!("Error opening file {}: {}", file_path.display(), e);
                track_ended.store(true, Ordering::SeqCst);
                return;
            }
        };
        
        // Extract and broadcast ID3 header
        let mut id3_buffer = vec![0; 16384];
        match file.read(&mut id3_buffer) {
            Ok(n) if n > 0 => {
                let id3_data = id3_buffer[..n].to_vec();
                
                if let Some(mut inner_lock) = inner.try_lock() {
                    inner_lock.id3_header = Some(id3_data.clone());
                    let _ = inner_lock.broadcast_tx.send(id3_data);
                }
                
                // Reset file position
                if let Err(e) = file.seek(SeekFrom::Start(0)) {
                    error!("Failed to seek to start: {}", e);
                    track_ended.store(true, Ordering::SeqCst);
                    return;
                }
            },
            _ => {
                error!("Failed to read ID3 header");
                track_ended.store(true, Ordering::SeqCst);
                return;
            }
        }
        
        // Better bitrate calculation
        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let duration = track.duration;
        let bitrate = if duration > 0 && file_size > 0 {
            (file_size * 8) / duration
        } else {
            128000 // Default to 128kbps
        };
        
        let bytes_per_second = bitrate / 8;
        let chunk_delay = Duration::from_millis(
            ((BROADCAST_CHUNK_SIZE as f64 / bytes_per_second as f64) * 1000.0) as u64
        ).max(Duration::from_millis(BROADCAST_RATE_LIMITER_MS));
        
        info!("Streaming {} at {}kbps, chunk delay: {}ms", 
             track.title, bitrate / 1000, chunk_delay.as_millis());
        
        // Stream the track
        let mut buffer = vec![0; BROADCAST_CHUNK_SIZE];
        let mut total_bytes_read = 0;
        let mut chunks_sent = 0;
        let mut last_chunk_time = Instant::now();
        
        loop {
            // Check if we should stop
            if should_stop.load(Ordering::SeqCst) || track_ended.load(Ordering::SeqCst) {
                break;
            }
            
            // Read chunk
            match file.read(&mut buffer) {
                Ok(0) => {
                    // End of file
                    info!("Reached end of file for track: {}", track.title);
                    break;
                },
                Ok(n) => {
                    let chunk = buffer[..n].to_vec();
                    total_bytes_read += n as u64;
                    
                    // Broadcast chunk
                    if let Some(mut inner_lock) = inner.try_lock() {
                        // Update playback position based on actual elapsed time
                        let elapsed = track_start.elapsed().as_secs();
                        inner_lock.playback_position = elapsed;
                        
                        // Save chunk for new clients
                        inner_lock.saved_chunks.push_back(chunk.clone());
                        while inner_lock.saved_chunks.len() > inner_lock.max_saved_chunks {
                            inner_lock.saved_chunks.pop_front();
                        }
                        
                        // Broadcast
                        let _ = inner_lock.broadcast_tx.send(chunk);
                        
                        // Log progress
                        if chunks_sent % 100 == 0 {
                            info!("Track {}: Sent {} chunks, position {}s of {}s", 
                                  track.title, chunks_sent, elapsed, track.duration);
                        }
                    }
                    
                    chunks_sent += 1;
                    
                    // Rate limiting
                    let elapsed_since_last = last_chunk_time.elapsed();
                    if elapsed_since_last < chunk_delay {
                        thread::sleep(chunk_delay - elapsed_since_last);
                    }
                    last_chunk_time = Instant::now();
                },
                Err(e) => {
                    error!("Error reading file: {}", e);
                    break;
                }
            }
        }
        
        // Wait for track duration to complete if we finished early
        let elapsed = track_start.elapsed().as_secs();
        if duration > 0 && elapsed < duration {
            let wait_time = duration - elapsed;
            info!("Track {} finished early. Waiting {}s to complete duration", 
                 track.title, wait_time);
            
            // Update position during wait
            let wait_start = Instant::now();
            while wait_start.elapsed().as_secs() < wait_time && 
                  !should_stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(1));
                
                // Update position
                if let Some(mut inner_lock) = inner.try_lock() {
                    let total_elapsed = track_start.elapsed().as_secs();
                    inner_lock.playback_position = total_elapsed;
                }
            }
        }
        
        info!("Track {} completed after {}s", track.title, track_start.elapsed().as_secs());
        
        // Mark track as ended
        track_ended.store(true, Ordering::SeqCst);
        
        // Send track end marker
        if let Some(mut inner_lock) = inner.try_lock() {
            let _ = inner_lock.broadcast_tx.send(vec![0xFF, 0xFF]);
        }
    }

    // Helper function to detect actual MP3 bitrate
    pub fn detect_mp3_bitrate(file: &mut File) -> Option<u64> {
        let mut buffer = vec![0; 8192];
        if let Ok(n) = file.read(&mut buffer) {
            // Simple MP3 frame detection
            for i in 0..n.saturating_sub(4) {
                if buffer[i] == 0xFF && (buffer[i + 1] & 0xE0) == 0xE0 {
                    // Found potential frame sync
                    let header = ((buffer[i + 1] as u32) << 16) | 
                                ((buffer[i + 2] as u32) << 8) | 
                                (buffer[i + 3] as u32);
                    
                    let bitrate_index = (header >> 12) & 0x0F;
                    let version = (header >> 19) & 0x03;
                    let layer = (header >> 17) & 0x03;
                    
                    // MPEG1 Layer 3 bitrates
                    if version == 3 && layer == 1 {
                        let bitrates = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
                        if (bitrate_index as usize) < bitrates.len() {
                            return Some(bitrates[bitrate_index as usize] as u64 * 1000);
                        }
                    }
                }
            }
        }
        None
    }
    
    // Helper function to calculate adaptive chunk delay
    pub fn calculate_chunk_delay(chunk_size: usize, bitrate: u64) -> Duration {
        let bytes_per_second = bitrate / 8;
        let seconds_per_chunk = chunk_size as f64 / bytes_per_second as f64;
        let ms_per_chunk = (seconds_per_chunk * 1000.0) as u64;
        
        // Add a small buffer to prevent underrun
        let buffered_ms = ms_per_chunk + 5;
        
        Duration::from_millis(buffered_ms.max(BROADCAST_RATE_LIMITER_MS))
    }
    
    // Simplified public interface
    pub fn start_streaming(&self, track_path: &str) {
        // The broadcast thread handles everything internally
        // This method is now just for compatibility
        info!("Start streaming requested for: {} (handled by broadcast thread)", track_path);
    }
    
    pub fn prepare_for_track_switch(&self) {
        // The broadcast thread handles transitions internally
        info!("Track switch preparation (handled by broadcast thread)");
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping broadcast explicitly");
        
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
    
    // All the other public methods remain the same
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
        let prev_count = self.active_listeners.load(Ordering::SeqCst);
        let new_count = self.active_listeners.fetch_add(1, Ordering::SeqCst) + 1;
        
        // Sanity check
        if new_count > 1000 {
            warn!("Suspicious listener count: {} (was {})", new_count, prev_count);
        }
        
        info!("Listener connected. Active listeners: {} -> {}", prev_count, new_count);
    }
    
    pub fn decrement_listener_count(&self) {
        let prev_count = self.active_listeners.load(Ordering::SeqCst);
        
        // Prevent underflow
        if prev_count == 0 {
            warn!("Attempted to decrement listener count below 0");
            return;
        }
        
        let new_count = self.active_listeners.fetch_sub(1, Ordering::SeqCst) - 1;
        info!("Listener disconnected. Active listeners: {} -> {}", prev_count, new_count);
    }
    
    // Add a method to check and fix listener count
    pub fn validate_listener_count(&self) {
        let current = self.active_listeners.load(Ordering::SeqCst);
        let receiver_count = self.broadcast_tx.receiver_count();
        
        if current != receiver_count {
            warn!("Listener count mismatch: {} vs {} receivers. Correcting...", 
                  current, receiver_count);
            self.active_listeners.store(receiver_count, Ordering::SeqCst);
        }
    }
    
    // Add a method to reset listener count
    pub fn reset_listener_count(&self) {
        let old_count = self.active_listeners.swap(0, Ordering::SeqCst);
        if old_count != 0 {
            warn!("Reset listener count from {} to 0", old_count);
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
    
    pub fn get_current_track_path(&self) -> Option<String> {
        self.inner.lock().current_track_path.clone()
    }
    
    pub fn is_stream_stalled(&self) -> bool {
        // With single thread design, stream is less likely to stall
        false
    }
    
    pub fn reset_track_ended_flag(&self) {
        self.track_ended.store(false, Ordering::SeqCst);
    }
    
    pub fn get_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
    
    pub fn get_saved_chunks_count(&self) -> usize {
        self.inner.lock().saved_chunks.len()
    }

    pub fn refresh_track_info(&self) {
        if let Some(track) = crate::services::playlist::get_current_track(
            &crate::config::PLAYLIST_FILE,
            &crate::config::MUSIC_FOLDER,
        ) {
            if let Ok(track_json) = serde_json::to_string(&track) {
                let mut inner = self.inner.lock();
                inner.current_track_info = Some(track_json);
            }
        }
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
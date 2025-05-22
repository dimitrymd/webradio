// src/services/streamer.rs - Enhanced version with precise position tracking

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, warn};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::path::PathBuf;

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
    active_listeners: Arc<AtomicUsize>,
    is_streaming: Arc<AtomicBool>,
    track_ended: Arc<AtomicBool>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    
    // Current track info
    current_track_info: Option<String>,
    
    // Enhanced playback position tracking
    playback_position: u64,           // Seconds
    playback_position_ms: u64,        // Milliseconds component
    track_start_time: Instant,        // When current track started
    position_last_updated: Instant,   // Last time position was calculated
    
    // Single broadcast thread
    broadcast_thread: Option<thread::JoinHandle<()>>,
    
    // Control flag for the broadcast thread
    should_stop: Arc<AtomicBool>,
    
    // Current bitrate and track metadata
    current_bitrate: u64,
    current_track_duration: u64,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager with enhanced position tracking");
        
        let should_stop = Arc::new(AtomicBool::new(false));
        let now = Instant::now();
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            current_track_info: None,
            playback_position: 0,
            playback_position_ms: 0,
            track_start_time: now,
            position_last_updated: now,
            broadcast_thread: None,
            should_stop: should_stop.clone(),
            current_bitrate: 128000, // Default starting bitrate
            current_track_duration: 0,
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
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
        
        info!("Starting enhanced track management thread");
        
        let thread_handle = thread::spawn(move || {
            Self::track_management_loop(
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
    
    fn track_management_loop(
        inner: Arc<Mutex<StreamManagerInner>>,
        is_streaming: Arc<AtomicBool>,
        track_ended: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        music_folder: &std::path::Path,
    ) {
        info!("Enhanced track management thread started");
        is_streaming.store(true, Ordering::SeqCst);
        
        let mut current_track_index: Option<usize> = None;
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get current playlist state
            let playlist = crate::services::playlist::get_playlist(&crate::config::PLAYLIST_FILE);
            
            // Determine which track to play
            let track_to_play = if let Some(index) = current_track_index {
                playlist.tracks.get(index).cloned()
            } else {
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
            info!("Managing track {}: {} by {} ({}s)", 
                 current_track_index.unwrap_or(0), track.title, track.artist, track.duration);
            
            // Initialize track info and position tracking
            {
                let mut inner_lock = inner.lock();
                let now = Instant::now();
                inner_lock.track_start_time = now;
                inner_lock.position_last_updated = now;
                inner_lock.playback_position = 0;
                inner_lock.playback_position_ms = 0;
                inner_lock.current_track_duration = track.duration;
                
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json);
                }
                
                // Calculate accurate bitrate if possible
                if let Ok(metadata) = std::fs::metadata(&track_path) {
                    let file_size = metadata.len();
                    if track.duration > 0 && file_size > 0 {
                        inner_lock.current_bitrate = (file_size * 8) / track.duration;
                        info!("Calculated bitrate: {} kbps", inner_lock.current_bitrate / 1000);
                    }
                }
            }
            
            // Reset track ended flag
            track_ended.store(false, Ordering::SeqCst);
            
            // Enhanced track playback with precise position updates
            let track_duration = track.duration;
            if track_duration > 0 {
                info!("Track \"{}\" will play for {} seconds with 100ms position updates", 
                      track.title, track_duration);
                
                let start_time = Instant::now();
                
                // Update position every 100ms for much better precision
                while start_time.elapsed().as_secs() < track_duration && !should_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                    
                    // Update playback position with millisecond precision
                    {
                        let mut inner_lock = inner.lock();
                        let elapsed = start_time.elapsed();
                        inner_lock.playback_position = elapsed.as_secs();
                        inner_lock.playback_position_ms = elapsed.subsec_millis() as u64;
                        inner_lock.position_last_updated = Instant::now();
                    }
                }
                
                // Final position update at track end
                {
                    let mut inner_lock = inner.lock();
                    inner_lock.playback_position = track_duration;
                    inner_lock.playback_position_ms = 0;
                }
            } else {
                // Fallback duration if track duration is unknown
                warn!("Track has no duration, using 180s fallback");
                let start_time = Instant::now();
                while start_time.elapsed().as_secs() < 180 && !should_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                    
                    {
                        let mut inner_lock = inner.lock();
                        let elapsed = start_time.elapsed();
                        inner_lock.playback_position = elapsed.as_secs();
                        inner_lock.playback_position_ms = elapsed.subsec_millis() as u64;
                        inner_lock.position_last_updated = Instant::now();
                    }
                }
            }
            
            // Track has ended
            if !should_stop.load(Ordering::SeqCst) {
                info!("Track {} finished at position {}s", track.title, track_duration);
                track_ended.store(true, Ordering::SeqCst);
                
                // Move to next track
                if let Some(index) = current_track_index {
                    if !playlist.tracks.is_empty() {
                        let next_index = (index + 1) % playlist.tracks.len();
                        current_track_index = Some(next_index);
                        info!("Advancing to track index: {}", next_index);
                        
                        // Update playlist file to reflect current position
                        let mut new_playlist = playlist.clone();
                        new_playlist.current_track = next_index;
                        crate::services::playlist::save_playlist(
                            &new_playlist, 
                            &crate::config::PLAYLIST_FILE
                        );
                    }
                }
                
                // Brief pause between tracks for smoother transitions
                thread::sleep(Duration::from_millis(500));
            }
        }
        
        info!("Enhanced track management thread ending");
    }
    
    // Enhanced API methods for direct streaming with precise position tracking
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
    
    // Enhanced position tracking with millisecond precision
    pub fn get_playback_position(&self) -> u64 {
        let inner = self.inner.lock();
        
        // Calculate real-time position based on elapsed time since track started
        let elapsed = inner.track_start_time.elapsed().as_secs();
        
        // Ensure we don't exceed track duration
        if inner.current_track_duration > 0 {
            elapsed.min(inner.current_track_duration)
        } else {
            elapsed
        }
    }
    
    // New method for precise position with milliseconds
    pub fn get_precise_position(&self) -> (u64, u64) {
        let inner = self.inner.lock();
        
        // Calculate real-time position with millisecond precision
        let elapsed = inner.track_start_time.elapsed();
        let total_seconds = elapsed.as_secs();
        let milliseconds = elapsed.subsec_millis() as u64;
        
        // Ensure we don't exceed track duration
        if inner.current_track_duration > 0 && total_seconds >= inner.current_track_duration {
            (inner.current_track_duration, 0)
        } else {
            (total_seconds, milliseconds)
        }
    }
    
    // Get position at a specific point in time (for better synchronization)
    pub fn get_position_at_time(&self, reference_time: Instant) -> (u64, u64) {
        let inner = self.inner.lock();
        
        // Calculate position based on reference time
        let elapsed_since_start = reference_time.duration_since(inner.track_start_time);
        let total_seconds = elapsed_since_start.as_secs();
        let milliseconds = elapsed_since_start.subsec_millis() as u64;
        
        // Bound by track duration
        if inner.current_track_duration > 0 && total_seconds >= inner.current_track_duration {
            (inner.current_track_duration, 0)
        } else {
            (total_seconds, milliseconds)
        }
    }
    
    pub fn get_current_bitrate(&self) -> u64 {
        self.inner.lock().current_bitrate
    }
    
    // New method to get track duration
    pub fn get_current_track_duration(&self) -> u64 {
        self.inner.lock().current_track_duration
    }
    
    // Method to check if position is near track end
    pub fn is_near_track_end(&self, threshold_seconds: u64) -> bool {
        let inner = self.inner.lock();
        if inner.current_track_duration == 0 {
            return false;
        }
        
        let elapsed = inner.track_start_time.elapsed().as_secs();
        elapsed + threshold_seconds >= inner.current_track_duration
    }
    
    // Method to estimate remaining track time
    pub fn get_remaining_time(&self) -> u64 {
        let inner = self.inner.lock();
        if inner.current_track_duration == 0 {
            return 0;
        }
        
        let elapsed = inner.track_start_time.elapsed().as_secs();
        inner.current_track_duration.saturating_sub(elapsed)
    }
    
    // Enhanced method to get comprehensive track state
    pub fn get_track_state(&self) -> TrackState {
        let inner = self.inner.lock();
        let elapsed = inner.track_start_time.elapsed();
        let position_secs = elapsed.as_secs();
        let position_ms = elapsed.subsec_millis() as u64;
        
        TrackState {
            position_seconds: if inner.current_track_duration > 0 {
                position_secs.min(inner.current_track_duration)
            } else {
                position_secs
            },
            position_milliseconds: position_ms,
            duration: inner.current_track_duration,
            bitrate: inner.current_bitrate,
            track_info: inner.current_track_info.clone(),
            is_near_end: inner.current_track_duration > 0 && 
                        position_secs + 10 >= inner.current_track_duration,
            remaining_time: inner.current_track_duration.saturating_sub(position_secs),
        }
    }
    
    pub fn stop_broadcasting(&self) {
        info!("Stopping enhanced track management");
        
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

// New struct for comprehensive track state
#[derive(Debug, Clone)]
pub struct TrackState {
    pub position_seconds: u64,
    pub position_milliseconds: u64,
    pub duration: u64,
    pub bitrate: u64,
    pub track_info: Option<String>,
    pub is_near_end: bool,
    pub remaining_time: u64,
}

impl Drop for StreamManager {
    fn drop(&mut self) {
        self.stop_broadcasting();
    }
}
// src/services/streamer.rs - Minimal version for direct streaming, fixed imports

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
    
    // Playback position
    playback_position: u64,
    track_start_time: Instant,
    
    // Single broadcast thread
    broadcast_thread: Option<thread::JoinHandle<()>>,
    
    // Control flag for the broadcast thread
    should_stop: Arc<AtomicBool>,
    
    // Current bitrate
    current_bitrate: u64,
}

impl StreamManager {
    pub fn new(music_folder: &std::path::Path, _chunk_size: usize, _buffer_size: usize, _cache_time: u64) -> Self {
        info!("Initializing StreamManager for direct streaming");
        
        let should_stop = Arc::new(AtomicBool::new(false));
        
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            current_track_info: None,
            playback_position: 0,
            track_start_time: Instant::now(),
            broadcast_thread: None,
            should_stop: should_stop.clone(),
            current_bitrate: 128000, // Default starting bitrate
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
        
        info!("Starting track management thread");
        
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
        info!("Track management thread started");
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
            info!("Managing track {}: {} by {}", 
                 current_track_index.unwrap_or(0), track.title, track.artist);
            
            // Update track info for direct streaming clients
            {
                let mut inner_lock = inner.lock();
                inner_lock.track_start_time = Instant::now();
                inner_lock.playback_position = 0;
                
                if let Ok(track_json) = serde_json::to_string(&track) {
                    inner_lock.current_track_info = Some(track_json);
                }
                
                // Calculate bitrate if possible
                if let Ok(metadata) = std::fs::metadata(&track_path) {
                    let file_size = metadata.len();
                    if track.duration > 0 && file_size > 0 {
                        inner_lock.current_bitrate = (file_size * 8) / track.duration;
                    }
                }
            }
            
            // Reset track ended flag
            track_ended.store(false, Ordering::SeqCst);
            
            // Simulate track playback duration
            let track_duration = track.duration;
            if track_duration > 0 {
                info!("Track \"{}\" will play for {} seconds", track.title, track_duration);
                
                // Update position periodically during track playback
                let start_time = Instant::now();
                while start_time.elapsed().as_secs() < track_duration && !should_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_secs(1));
                    
                    // Update playback position
                    {
                        let mut inner_lock = inner.lock();
                        inner_lock.playback_position = start_time.elapsed().as_secs();
                    }
                }
            } else {
                // Fallback duration if track duration is unknown
                thread::sleep(Duration::from_secs(180)); // 3 minutes default
            }
            
            // Track has ended
            if !should_stop.load(Ordering::SeqCst) {
                info!("Track {} finished", track.title);
                track_ended.store(true, Ordering::SeqCst);
                
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
        
        info!("Track management thread ending");
    }
    
    // API methods for direct streaming
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
        info!("Stopping track management");
        
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
        self.stop_broadcasting();
    }
}
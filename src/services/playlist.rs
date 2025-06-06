// src/services/playlist.rs - Fully optimized with file watching and caching

use std::path::Path;
use std::time::Duration;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::interval;
use parking_lot::RwLock;
use notify::{Watcher, RecursiveMode, watcher, DebouncedEvent};
use std::sync::mpsc::channel;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Longer intervals for less CPU usage
const STATUS_LOG_INTERVAL_SECS: u64 = 600;    // 10 minutes
const CLEANUP_INTERVAL_SECS: u64 = 600;       // 10 minutes

// Global playlist cache with file watcher
lazy_static::lazy_static! {
    static ref PLAYLIST_CACHE: Arc<RwLock<Option<Playlist>>> = Arc::new(RwLock::new(None));
    static ref PLAYLIST_WATCHER: Arc<RwLock<Option<PlaylistWatcher>>> = Arc::new(RwLock::new(None));
}

// Track duration cache
static mut DURATION_CACHE: Option<std::collections::HashMap<String, u64>> = None;
static mut DURATION_CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());

struct PlaylistWatcher {
    _watcher: notify::FsEventWatcher,
    _thread: std::thread::JoinHandle<()>,
}

// Initialize the file watcher for the playlist
pub fn init_playlist_watcher(playlist_file: &Path) {
    let playlist_path = playlist_file.to_path_buf();
    let playlist_dir = playlist_file.parent().unwrap_or(Path::new(".")).to_path_buf();
    
    // Load initial playlist
    let initial_playlist = read_playlist_from_file(&playlist_path);
    *PLAYLIST_CACHE.write() = Some(initial_playlist);
    
    // Set up file watcher
    let (tx, rx) = channel();
    let mut watcher = watcher(tx, Duration::from_secs(1))
        .expect("Failed to create file watcher");
    
    watcher.watch(&playlist_dir, RecursiveMode::NonRecursive)
        .expect("Failed to watch playlist directory");
    
    let playlist_path_clone = playlist_path.clone();
    let thread = std::thread::spawn(move || {
        loop {
            match rx.recv() {
                Ok(DebouncedEvent::Write(path)) | Ok(DebouncedEvent::Create(path)) => {
                    if path == playlist_path_clone {
                        log::info!("Playlist file changed, reloading...");
                        let new_playlist = read_playlist_from_file(&playlist_path_clone);
                        *PLAYLIST_CACHE.write() = Some(new_playlist);
                    }
                }
                Err(e) => {
                    log::error!("Watcher error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }
    });
    
    *PLAYLIST_WATCHER.write() = Some(PlaylistWatcher {
        _watcher: watcher,
        _thread: thread,
    });
}

// Get playlist from cache (no file I/O)
pub fn get_playlist_cached() -> Playlist {
    PLAYLIST_CACHE.read()
        .as_ref()
        .cloned()
        .unwrap_or_default()
}

// For compatibility
pub fn get_playlist(playlist_file: &Path) -> Playlist {
    // Initialize watcher on first access
    if PLAYLIST_WATCHER.read().is_none() {
        init_playlist_watcher(playlist_file);
    }
    
    get_playlist_cached()
}

fn read_playlist_from_file(playlist_file: &Path) -> Playlist {
    if playlist_file.exists() {
        match std::fs::read_to_string(playlist_file) {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(playlist) => return playlist,
                    Err(e) => {
                        log::error!("Error parsing playlist file: {}", e);
                    }
                }
            }
            Err(e) => {
                log::error!("Error reading playlist file: {}", e);
            }
        }
    }
    
    Playlist::default()
}

// Async version for saving playlist
pub async fn save_playlist_async(playlist: &Playlist, playlist_file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = playlist_file.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    let json = serde_json::to_string_pretty(playlist)?;
    
    let mut file = fs::File::create(playlist_file).await?;
    file.write_all(json.as_bytes()).await?;
    file.flush().await?;
    
    Ok(())
}

// Keep sync version for compatibility
pub fn save_playlist(playlist: &Playlist, playlist_file: &Path) {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            if let Err(e) = save_playlist_async(playlist, playlist_file).await {
                log::error!("Error saving playlist: {}", e);
            }
        })
    });
}

pub fn get_current_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    if playlist.tracks.is_empty() {
        return None;
    }
    
    let current_index = if playlist.current_track >= playlist.tracks.len() {
        0
    } else {
        playlist.current_track
    };
    
    let current_track = playlist.tracks.get(current_index)?;
    
    // Check if file exists (still sync for now)
    let track_path = music_folder.join(&current_track.path);
    if !track_path.exists() {
        log::warn!("Current track file not found: {}", current_track.path);
        return None;
    }
    
    Some(current_track.clone())
}

// Async version of scan_music_folder
pub async fn scan_music_folder_async(music_folder: &Path, playlist_file: &Path) -> Result<Playlist, Box<dyn std::error::Error>> {
    // Still use sync mp3_scanner for now
    let mp3_files = tokio::task::spawn_blocking({
        let music_folder = music_folder.to_path_buf();
        move || mp3_scanner::scan_directory(&music_folder)
    }).await?;
    
    let mut playlist = get_playlist(playlist_file);
    
    let existing_paths: Vec<String> = playlist.tracks.iter()
        .map(|track| track.path.clone())
        .collect();
    
    let mut added_count = 0;
    for mp3 in mp3_files {
        if !existing_paths.contains(&mp3.path) {
            playlist.tracks.push(mp3);
            added_count += 1;
        }
    }
    
    // Async file existence check for cleanup
    let mut tracks_to_keep = Vec::new();
    for track in &playlist.tracks {
        let track_path = music_folder.join(&track.path);
        if fs::metadata(&track_path).await.is_ok() {
            tracks_to_keep.push(track.clone());
        }
    }
    
    let removed_count = playlist.tracks.len() - tracks_to_keep.len();
    playlist.tracks = tracks_to_keep;
    
    if removed_count > 0 {
        log::info!("Removed {} missing tracks", removed_count);
    }
    
    if added_count > 0 {
        log::info!("Added {} new tracks", added_count);
    }
    
    if !playlist.tracks.is_empty() && playlist.current_track >= playlist.tracks.len() {
        playlist.current_track = 0;
    }
    
    save_playlist_async(&playlist, playlist_file).await?;
    Ok(playlist)
}

// Keep sync version for compatibility
pub fn scan_music_folder(music_folder: &Path, playlist_file: &Path) -> Playlist {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            match scan_music_folder_async(music_folder, playlist_file).await {
                Ok(playlist) => playlist,
                Err(e) => {
                    log::error!("Error scanning music folder: {}", e);
                    Playlist::default()
                }
            }
        })
    })
}

// Async track duration updater with caching
pub async fn rescan_and_update_durations_async(playlist_file: &Path, music_folder: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut playlist = get_playlist(playlist_file);
    let mut updated_count = 0;
    
    // Initialize duration cache
    unsafe {
        let _lock = DURATION_CACHE_MUTEX.lock();
        if DURATION_CACHE.is_none() {
            DURATION_CACHE = Some(std::collections::HashMap::new());
        }
    }
    
    for track in &mut playlist.tracks {
        let file_path = music_folder.join(&track.path);
        
        // Check if file exists asynchronously
        if fs::metadata(&file_path).await.is_err() {
            continue;
        }
        
        // Check duration cache first
        let cached_duration = unsafe {
            let _lock = DURATION_CACHE_MUTEX.lock();
            DURATION_CACHE.as_ref().unwrap().get(&track.path).copied()
        };
        
        if let Some(duration) = cached_duration {
            if track.duration != duration {
                track.duration = duration;
                updated_count += 1;
            }
            continue;
        }
        
        // Calculate duration in blocking task
        let file_path_clone = file_path.clone();
        let duration_result = tokio::task::spawn_blocking(move || {
            mp3_duration::from_path(&file_path_clone)
        }).await?;
        
        match duration_result {
            Ok(d) => {
                let new_duration = d.as_secs();
                
                if new_duration != track.duration && new_duration > 0 {
                    track.duration = new_duration;
                    updated_count += 1;
                    
                    // Update cache
                    unsafe {
                        let _lock = DURATION_CACHE_MUTEX.lock();
                        DURATION_CACHE.as_mut().unwrap().insert(track.path.clone(), new_duration);
                    }
                }
            },
            Err(_) => {
                // Fallback: estimate from file size
                if let Ok(metadata) = fs::metadata(&file_path).await {
                    let file_size = metadata.len();
                    let estimated_duration = file_size / 16000;
                    if estimated_duration > 0 && estimated_duration != track.duration {
                        track.duration = estimated_duration;
                        updated_count += 1;
                        
                        // Update cache
                        unsafe {
                            let _lock = DURATION_CACHE_MUTEX.lock();
                            DURATION_CACHE.as_mut().unwrap().insert(track.path.clone(), estimated_duration);
                        }
                    }
                }
            }
        }
    }
    
    if updated_count > 0 {
        log::info!("Updated {} track durations", updated_count);
        save_playlist_async(&playlist, playlist_file).await?;
    }
    
    Ok(())
}

// Keep sync version for compatibility
pub fn rescan_and_update_durations(playlist_file: &Path, music_folder: &Path) {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            if let Err(e) = rescan_and_update_durations_async(playlist_file, music_folder).await {
                log::error!("Error updating durations: {}", e);
            }
        })
    });
}

// Optimized async monitor function
pub async fn track_switcher_async(stream_manager: Arc<StreamManager>) {
    log::info!("Starting optimized async monitor");
    
    let mut status_interval = interval(Duration::from_secs(STATUS_LOG_INTERVAL_SECS));
    let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
    
    // Set missed tick behavior to avoid accumulating ticks
    status_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    cleanup_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    
    loop {
        tokio::select! {
            _ = status_interval.tick() => {
                let active_listeners = stream_manager.get_active_listeners();
                if active_listeners > 0 {
                    log::info!("Active listeners: {}", active_listeners);
                }
            }
            
            _ = cleanup_interval.tick() => {
                stream_manager.cleanup_stale_connections();
            }
        }
    }
}

// Sync wrapper for compatibility
pub fn track_switcher(stream_manager: Arc<StreamManager>) {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(track_switcher_async(stream_manager))
    });
}

pub fn get_next_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    if playlist.tracks.is_empty() {
        return None;
    }
    
    let next_index = (playlist.current_track + 1) % playlist.tracks.len();
    let next_track = playlist.tracks.get(next_index)?;
    
    // Check if file exists
    let track_path = music_folder.join(&next_track.path);
    if !track_path.exists() {
        return None;
    }
    
    Some(next_track.clone())
}

pub fn verify_track_durations(_playlist_file: &Path, _music_folder: &Path) {
    // Disabled for CPU optimization
    log::debug!("Track duration verification disabled for CPU optimization");
}

// No need for invalidate_playlist_cache anymore as the watcher handles it
// src/services/playlist.rs - Async I/O implementation

use std::path::Path;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::interval;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Longer intervals for less CPU usage
const STATUS_LOG_INTERVAL_SECS: u64 = 600;    // 10 minutes
const CLEANUP_INTERVAL_SECS: u64 = 600;       // 10 minutes
const FILE_SCAN_INTERVAL_SECS: u64 = 7200;    // 2 hours

// Enhanced caching with longer TTL
static mut CACHED_PLAYLIST: Option<(Playlist, Instant)> = None;
static mut CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());
const CACHE_TTL_SECS: u64 = 60; // 1 minute cache

// Track duration cache to avoid repeated mp3_duration calls
static mut DURATION_CACHE: Option<std::collections::HashMap<String, u64>> = None;
static mut DURATION_CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());

pub fn invalidate_playlist_cache() {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        CACHED_PLAYLIST = None;
    }
}

pub fn get_playlist(playlist_file: &Path) -> Playlist {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        
        // Check cache first
        if let Some((ref cached_playlist, cache_time)) = CACHED_PLAYLIST {
            if cache_time.elapsed().as_secs() < CACHE_TTL_SECS {
                return cached_playlist.clone();
            }
        }
        
        // Read from file only if cache miss
        let playlist = read_playlist_from_file(playlist_file);
        CACHED_PLAYLIST = Some((playlist.clone(), Instant::now()));
        playlist
    }
}

fn read_playlist_from_file(playlist_file: &Path) -> Playlist {
    // Use blocking read for now, but could be converted to async
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
    // Only save if changed - compare with cache
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        if let Some((ref cached_playlist, _)) = CACHED_PLAYLIST {
            if cached_playlist.current_track == playlist.current_track &&
               cached_playlist.tracks.len() == playlist.tracks.len() {
                // No significant changes, skip save
                return Ok(());
            }
        }
    }
    
    if let Some(parent) = playlist_file.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    let json = serde_json::to_string_pretty(playlist)?;
    
    let mut file = fs::File::create(playlist_file).await?;
    file.write_all(json.as_bytes()).await?;
    file.flush().await?;
    
    invalidate_playlist_cache();
    Ok(())
}

// Keep sync version for compatibility
pub fn save_playlist(playlist: &Playlist, playlist_file: &Path) {
    // Use tokio's block_in_place for sync context
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

// Async track duration updater
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

// Async monitor function
pub async fn track_switcher_async(stream_manager: Arc<StreamManager>) {
    log::info!("Starting async monitor");
    
    let mut status_interval = interval(Duration::from_secs(STATUS_LOG_INTERVAL_SECS));
    let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
    let mut file_scan_interval = interval(Duration::from_secs(FILE_SCAN_INTERVAL_SECS));
    
    // Set missed tick behavior to avoid accumulating ticks
    status_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    cleanup_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    file_scan_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    
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
            
            _ = file_scan_interval.tick() => {
                let playlist = get_playlist(&crate::config::PLAYLIST_FILE);
                
                if playlist.tracks.is_empty() {
                    if let Err(e) = scan_music_folder_async(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE).await {
                        log::error!("Error scanning music folder: {}", e);
                    }
                }
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
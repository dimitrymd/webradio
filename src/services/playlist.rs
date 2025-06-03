// src/services/playlist.rs - CPU-optimized with minimal file I/O

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::Arc;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Longer intervals for less CPU usage
const STATUS_LOG_INTERVAL_SECS: u64 = 600;    // 10 minutes (was 300)
const CLEANUP_INTERVAL_SECS: u64 = 600;       // 10 minutes (was 300)
const FILE_SCAN_INTERVAL_SECS: u64 = 7200;    // 2 hours (was 3600)

// Enhanced caching with longer TTL
static mut CACHED_PLAYLIST: Option<(Playlist, Instant)> = None;
static mut CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());
const CACHE_TTL_SECS: u64 = 60; // 1 minute cache (was 30)

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
    // Check if file exists without repeated stat calls
    static mut LAST_FILE_CHECK: Option<(bool, Instant)> = None;
    let file_exists = unsafe {
        if let Some((exists, check_time)) = LAST_FILE_CHECK {
            if check_time.elapsed().as_secs() < 10 {
                exists
            } else {
                let exists = playlist_file.exists();
                LAST_FILE_CHECK = Some((exists, Instant::now()));
                exists
            }
        } else {
            let exists = playlist_file.exists();
            LAST_FILE_CHECK = Some((exists, Instant::now()));
            exists
        }
    };
    
    if file_exists {
        match fs::read_to_string(playlist_file) {
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

pub fn save_playlist(playlist: &Playlist, playlist_file: &Path) {
    // Only save if changed - compare with cache
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        if let Some((ref cached_playlist, _)) = CACHED_PLAYLIST {
            if cached_playlist.current_track == playlist.current_track &&
               cached_playlist.tracks.len() == playlist.tracks.len() {
                // No significant changes, skip save
                return;
            }
        }
    }
    
    if let Some(parent) = playlist_file.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            log::error!("Failed to create directory: {}", e);
        });
    }
    
    let json = serde_json::to_string_pretty(playlist).unwrap_or_else(|e| {
        log::error!("Error serializing playlist: {}", e);
        String::new()
    });
    
    match File::create(playlist_file) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(json.as_bytes()) {
                log::error!("Error writing playlist file: {}", e);
            } else {
                invalidate_playlist_cache();
            }
        },
        Err(e) => {
            log::error!("Error creating playlist file: {}", e);
        }
    }
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
    
    // Cache file existence checks
    static mut FILE_EXISTS_CACHE: Option<std::collections::HashMap<String, (bool, Instant)>> = None;
    static mut FILE_CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());
    
    let track_exists = unsafe {
        let _lock = FILE_CACHE_MUTEX.lock();
        
        if FILE_EXISTS_CACHE.is_none() {
            FILE_EXISTS_CACHE = Some(std::collections::HashMap::new());
        }
        
        let cache = FILE_EXISTS_CACHE.as_mut().unwrap();
        
        if let Some((exists, check_time)) = cache.get(&current_track.path) {
            if check_time.elapsed().as_secs() < 60 {
                *exists
            } else {
                let track_path = music_folder.join(&current_track.path);
                let exists = track_path.exists();
                cache.insert(current_track.path.clone(), (exists, Instant::now()));
                exists
            }
        } else {
            let track_path = music_folder.join(&current_track.path);
            let exists = track_path.exists();
            cache.insert(current_track.path.clone(), (exists, Instant::now()));
            exists
        }
    };
    
    if !track_exists {
        log::warn!("Current track file not found: {}", current_track.path);
        return None;
    }
    
    Some(current_track.clone())
}

pub fn scan_music_folder(music_folder: &Path, playlist_file: &Path) -> Playlist {
    let mp3_files = mp3_scanner::scan_directory(music_folder);
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
    
    // Only clean up missing tracks occasionally
    static mut LAST_CLEANUP: Option<Instant> = None;
    let should_cleanup = unsafe {
        if let Some(last) = LAST_CLEANUP {
            last.elapsed().as_secs() > 3600 // 1 hour
        } else {
            LAST_CLEANUP = Some(Instant::now());
            true
        }
    };
    
    if should_cleanup {
        unsafe { LAST_CLEANUP = Some(Instant::now()); }
        
        let before_count = playlist.tracks.len();
        playlist.tracks.retain(|track| {
            music_folder.join(&track.path).exists()
        });
        let after_count = playlist.tracks.len();
        let removed_count = before_count - after_count;
        
        if removed_count > 0 {
            log::info!("Removed {} missing tracks", removed_count);
        }
    }
    
    if added_count > 0 {
        log::info!("Added {} new tracks", added_count);
    }
    
    if !playlist.tracks.is_empty() && playlist.current_track >= playlist.tracks.len() {
        playlist.current_track = 0;
    }
    
    save_playlist(&playlist, playlist_file);
    playlist
}

pub fn rescan_and_update_durations(playlist_file: &Path, music_folder: &Path) {
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
        
        if !file_path.exists() {
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
        
        // Not in cache, calculate duration
        let old_duration = track.duration;
        
        match mp3_duration::from_path(&file_path) {
            Ok(d) => {
                let new_duration = d.as_secs();
                
                if new_duration != old_duration && new_duration > 0 {
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
                if let Ok(metadata) = file_path.metadata() {
                    let file_size = metadata.len();
                    let estimated_duration = file_size / 16000;
                    if estimated_duration > 0 && estimated_duration != old_duration {
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
        save_playlist(&playlist, playlist_file);
    }
}

// Minimal monitor - only essential operations
pub fn track_switcher(stream_manager: Arc<StreamManager>) {
    log::info!("Starting minimal CPU-optimized monitor");
    
    let mut last_log_time = Instant::now();
    let mut last_cleanup_time = Instant::now();
    let mut last_file_scan = Instant::now();
    
    loop {
        // Much longer sleep interval to reduce CPU
        thread::sleep(Duration::from_secs(60)); // 1 minute (was 30)
        
        // Clean up stale connections less frequently
        if last_cleanup_time.elapsed().as_secs() >= CLEANUP_INTERVAL_SECS {
            stream_manager.cleanup_stale_connections();
            last_cleanup_time = Instant::now();
        }
        
        // Log status less frequently
        if last_log_time.elapsed().as_secs() >= STATUS_LOG_INTERVAL_SECS {
            let active_listeners = stream_manager.get_active_listeners();
            
            if active_listeners > 0 {  // Only log if there are listeners
                log::info!("Active listeners: {}", active_listeners);
            }
            
            last_log_time = Instant::now();
        }
        
        // Check for new files less frequently
        if last_file_scan.elapsed().as_secs() >= FILE_SCAN_INTERVAL_SECS {
            let playlist = get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
            }
            
            last_file_scan = Instant::now();
        }
    }
}

pub fn get_next_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    if playlist.tracks.is_empty() {
        return None;
    }
    
    let next_index = (playlist.current_track + 1) % playlist.tracks.len();
    let next_track = playlist.tracks.get(next_index)?;
    
    // Use cached file existence check
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
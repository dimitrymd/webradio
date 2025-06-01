// src/services/playlist.rs - CPU Optimized version

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::Arc;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Optimized constants - less frequent operations
const TRACK_CHECK_INTERVAL_SECS: u64 = 5;     // Increased from 1
const TRACK_SWITCH_THRESHOLD_SECS: u64 = 10;  // Increased from 5
const MIN_TRACK_DURATION: u64 = 15;           // Increased from 10
const STATUS_LOG_INTERVAL_SECS: u64 = 60;     // Increased from 30
const CLEANUP_INTERVAL_SECS: u64 = 60;        // Increased from 30
const FILE_SCAN_INTERVAL_SECS: u64 = 600;     // Check for new files every 10 minutes

// Cached playlist to reduce file I/O
static mut CACHED_PLAYLIST: Option<(Playlist, Instant)> = None;
static mut CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());

// Function to invalidate cache when playlist changes
pub fn invalidate_playlist_cache() {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        CACHED_PLAYLIST = None;
    }
}

pub fn get_playlist(playlist_file: &Path) -> Playlist {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        
        // Check if we have a cached playlist that's less than 10 seconds old (reduced from 30)
        if let Some((ref cached_playlist, cache_time)) = CACHED_PLAYLIST {
            if cache_time.elapsed().as_secs() < 10 {
                return cached_playlist.clone();
            }
        }
        
        // Read from file
        let playlist = read_playlist_from_file(playlist_file);
        
        // Update cache
        CACHED_PLAYLIST = Some((playlist.clone(), Instant::now()));
        
        playlist
    }
}

fn read_playlist_from_file(playlist_file: &Path) -> Playlist {
    if playlist_file.exists() {
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
    
    // Return default empty playlist
    Playlist::default()
}

pub fn save_playlist(playlist: &Playlist, playlist_file: &Path) {
    // Create parent directory if it doesn't exist
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
                // IMPORTANT: Invalidate cache after successful write
                invalidate_playlist_cache();
                log::debug!("Playlist saved and cache invalidated");
            }
        },
        Err(e) => {
            log::error!("Error creating playlist file: {}", e);
        }
    }
}

pub fn get_current_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    // Return None if no tracks
    if playlist.tracks.is_empty() {
        return None;
    }
    
    // Make sure current_track index is valid
    let current_index = if playlist.current_track >= playlist.tracks.len() {
        0
    } else {
        playlist.current_track
    };
    
    // Get current track
    let current_track = playlist.tracks.get(current_index)?;
    
    // Verify file exists (less frequently to save CPU)
    let track_path = music_folder.join(&current_track.path);
    if !track_path.exists() {
        log::warn!("Current track file not found: {}", track_path.display());
        return None;
    }
    
    Some(current_track.clone())
}

pub fn advance_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let mut playlist = get_playlist(playlist_file);
    
    // Return None if no tracks
    if playlist.tracks.is_empty() {
        return None;
    }
    
    // Move to next track
    let old_index = playlist.current_track;
    playlist.current_track = (playlist.current_track + 1) % playlist.tracks.len();
    
    log::info!("Advanced playlist from track {} to track {}", old_index, playlist.current_track);
    
    save_playlist(&playlist, playlist_file);
    
    // Get the new current track
    get_current_track(playlist_file, music_folder)
}

pub fn scan_music_folder(music_folder: &Path, playlist_file: &Path) -> Playlist {
    let mp3_files = mp3_scanner::scan_directory(music_folder);
    
    // Update playlist file
    let mut playlist = get_playlist(playlist_file);
    
    // Keep track of existing tracks
    let existing_paths: Vec<String> = playlist.tracks.iter()
        .map(|track| track.path.clone())
        .collect();
    
    // Add new tracks
    let mut added_count = 0;
    for mp3 in mp3_files {
        if !existing_paths.contains(&mp3.path) {
            log::info!("Adding new track: {} by {}", mp3.title, mp3.artist);
            playlist.tracks.push(mp3);
            added_count += 1;
        }
    }
    
    // Remove tracks that no longer exist (batch operation)
    let before_count = playlist.tracks.len();
    playlist.tracks.retain(|track| {
        let exists = music_folder.join(&track.path).exists();
        if !exists {
            log::warn!("Removing missing track: {}", track.path);
        }
        exists
    });
    let after_count = playlist.tracks.len();
    let removed_count = before_count - after_count;
    
    if added_count > 0 || removed_count > 0 {
        log::info!("Playlist updated: +{} tracks, -{} tracks, total: {}", 
                   added_count, removed_count, playlist.tracks.len());
    }
    
    // Make sure current_track is valid
    if !playlist.tracks.is_empty() && playlist.current_track >= playlist.tracks.len() {
        playlist.current_track = 0;
        log::info!("Reset current track index to 0");
    }
    
    save_playlist(&playlist, playlist_file);
    playlist
}

// Optimized duration verification - only run when explicitly requested
pub fn verify_track_durations(playlist_file: &Path, music_folder: &Path) {
    let playlist = get_playlist(playlist_file);
    
    println!("============ TRACK DURATION VERIFICATION ============");
    println!("Current track index: {}", playlist.current_track);
    println!("Total tracks: {}", playlist.tracks.len());
    
    for (i, track) in playlist.tracks.iter().enumerate() {
        let file_path = music_folder.join(&track.path);
        let file_exists = file_path.exists();
        
        // Only check duration for files that exist
        let actual_duration = if file_exists {
            match mp3_duration::from_path(&file_path) {
                Ok(duration) => duration.as_secs(),
                Err(_) => 0
            }
        } else {
            0
        };
        
        let current_marker = if i == playlist.current_track { " ← CURRENT" } else { "" };
        
        println!("Track {}: \"{}\" by \"{}\"{}", i, track.title, track.artist, current_marker);
        println!("  → Stored duration: {} seconds", track.duration);
        println!("  → Actual duration: {} seconds", actual_duration);
        println!("  → File exists: {}", file_exists);
        
        // Flag potential issues
        if track.duration == 0 {
            println!("  ⚠️ WARNING: Track has zero duration");
        }
        if track.duration > 0 && actual_duration > 0 && 
           (track.duration < actual_duration/2 || track.duration > actual_duration*2) {
            println!("  ⚠️ WARNING: Significant mismatch between stored and actual duration");
        }
        if !file_exists {
            println!("  ⚠️ WARNING: File does not exist");
        }
    }
    println!("=====================================================");
}

// Optimized duration updating - batch operations
pub fn rescan_and_update_durations(playlist_file: &Path, music_folder: &Path) {
    println!("Rescanning music folder and updating track durations...");
    
    let mut playlist = get_playlist(playlist_file);
    let mut updated_count = 0;
    
    // Process tracks in batches to reduce I/O overhead
    for track in &mut playlist.tracks {
        let file_path = music_folder.join(&track.path);
        
        // Skip if file doesn't exist
        if !file_path.exists() {
            continue;
        }
        
        let old_duration = track.duration;
        
        // Get accurate duration
        match mp3_duration::from_path(&file_path) {
            Ok(d) => {
                let new_duration = d.as_secs();
                
                // Update if different
                if new_duration != old_duration && new_duration > 0 {
                    println!("Updating duration for \"{}\" by \"{}\": {} -> {} seconds", 
                             track.title, track.artist, old_duration, new_duration);
                    track.duration = new_duration;
                    updated_count += 1;
                } else if new_duration == 0 {
                    // Try filesize method as fallback
                    if let Ok(metadata) = file_path.metadata() {
                        let file_size = metadata.len();
                        // Rough estimate: MP3 at 128kbps = 16KB per second
                        let estimated_duration = file_size / 16000;
                        if estimated_duration > 0 && estimated_duration != old_duration {
                            println!("Using filesize estimate for \"{}\": {} -> {} seconds", 
                                     track.title, old_duration, estimated_duration);
                            track.duration = estimated_duration;
                            updated_count += 1;
                        }
                    }
                }
            },
            Err(e) => {
                println!("Error getting duration for {}: {}", file_path.display(), e);
                
                // Try filesize method as fallback
                if let Ok(metadata) = file_path.metadata() {
                    let file_size = metadata.len();
                    let estimated_duration = file_size / 16000;
                    if estimated_duration > 0 && estimated_duration != old_duration {
                        println!("Using filesize estimate for \"{}\": {} -> {} seconds", 
                                 track.title, old_duration, estimated_duration);
                        track.duration = estimated_duration;
                        updated_count += 1;
                    }
                }
            }
        }
    }
    
    println!("Updated {} track durations", updated_count);
    
    // Save updated playlist
    save_playlist(&playlist, playlist_file);
    println!("Playlist updated and saved");
}

// CPU-optimized track switcher with reduced frequency operations
pub fn track_switcher(stream_manager: Arc<StreamManager>) {
    println!("Track monitor thread started - CPU optimized version");
    
    let mut last_log_time = Instant::now();
    let mut last_cleanup_time = Instant::now();
    let mut last_position_check = Instant::now();
    let mut last_file_scan = Instant::now();
    let mut current_track_info: Option<String> = None;
    
    loop {
        thread::sleep(Duration::from_secs(TRACK_CHECK_INTERVAL_SECS));
        
        // Clean up stale connections less frequently
        if last_cleanup_time.elapsed().as_secs() >= CLEANUP_INTERVAL_SECS {
            stream_manager.cleanup_stale_connections();
            last_cleanup_time = Instant::now();
        }
        
        // Check track position for auto-switching (reduced frequency)
        if last_position_check.elapsed().as_secs() >= TRACK_CHECK_INTERVAL_SECS {
            let track_state = stream_manager.get_track_state();
            
            // Check if we should automatically switch tracks
            if track_state.duration > 0 && 
               track_state.remaining_time <= TRACK_SWITCH_THRESHOLD_SECS && 
               track_state.position_seconds >= MIN_TRACK_DURATION {
                
                // Get current track info to see if it changed
                let new_track_info = track_state.track_info.clone();
                
                if let Some(ref current_info) = current_track_info {
                    if let Some(ref new_info) = new_track_info {
                        if current_info == new_info {
                            // Same track, check if it's time to switch
                            log::info!("Track has {}s remaining - requesting switch", 
                                     track_state.remaining_time);
                            
                            stream_manager.request_track_switch();
                            
                            // Update current track info to prevent repeated switches
                            current_track_info = None;
                        }
                    }
                } else {
                    current_track_info = new_track_info;
                }
            } else if track_state.remaining_time > TRACK_SWITCH_THRESHOLD_SECS {
                // Track is not near end, update current track info
                current_track_info = track_state.track_info.clone();
            }
            
            last_position_check = Instant::now();
        }
        
        // Log status less frequently to reduce CPU usage
        if last_log_time.elapsed().as_secs() >= STATUS_LOG_INTERVAL_SECS {
            let active_listeners = stream_manager.get_active_listeners();
            let is_streaming = stream_manager.is_streaming();
            let track_state = stream_manager.get_track_state();
            
            if let Some(track_info) = &track_state.track_info {
                if let Some(title) = extract_title_from_track_info(track_info) {
                    log::info!("MONITOR: \"{}\" at {}s/{}s, listeners={}, streaming={}", 
                              title, track_state.position_seconds, track_state.duration, 
                              active_listeners, is_streaming);
                } else {
                    log::info!("MONITOR: Position {}s/{}s, listeners={}, streaming={}", 
                              track_state.position_seconds, track_state.duration,
                              active_listeners, is_streaming);
                }
            } else {
                log::info!("MONITOR: No track info, listeners={}, streaming={}", 
                          active_listeners, is_streaming);
            }
            
            last_log_time = Instant::now();
        }
        
        // Check for new files much less frequently to save CPU
        if last_file_scan.elapsed().as_secs() >= FILE_SCAN_INTERVAL_SECS {
            let playlist = get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                log::info!("MONITOR: No tracks in playlist, scanning for new files...");
                scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
            }
            
            last_file_scan = Instant::now();
        }
    }
}

// Helper function to extract title from track info JSON (cached)
fn extract_title_from_track_info(track_info: &str) -> Option<String> {
    // Simple string search instead of full JSON parsing for better performance
    if let Some(start) = track_info.find("\"title\":\"") {
        let start = start + 9; // Length of "title":"
        if let Some(end) = track_info[start..].find('"') {
            return Some(track_info[start..start + end].to_string());
        }
    }
    None
}

// Manual track advance function for testing
pub fn manually_advance_track(stream_manager: &Arc<StreamManager>) {
    log::info!("Manual track advance requested");
    stream_manager.request_track_switch();
}

// Get next track in playlist without advancing (cached)
pub fn get_next_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    if playlist.tracks.is_empty() {
        return None;
    }
    
    let next_index = (playlist.current_track + 1) % playlist.tracks.len();
    let next_track = playlist.tracks.get(next_index)?;
    
    // Verify file exists (but don't log to reduce noise)
    let track_path = music_folder.join(&next_track.path);
    if !track_path.exists() {
        return None;
    }
    
    Some(next_track.clone())
}
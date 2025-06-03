// src/services/playlist.rs - Minimal monitor (no track control)

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::Arc;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Constants for monitoring only
const STATUS_LOG_INTERVAL_SECS: u64 = 60;     // Log status every minute
const CLEANUP_INTERVAL_SECS: u64 = 60;        // Cleanup every minute
const FILE_SCAN_INTERVAL_SECS: u64 = 600;     // Scan for new files every 10 minutes

// Cached playlist to reduce file I/O
static mut CACHED_PLAYLIST: Option<(Playlist, Instant)> = None;
static mut CACHE_MUTEX: parking_lot::Mutex<()> = parking_lot::const_mutex(());

pub fn invalidate_playlist_cache() {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        CACHED_PLAYLIST = None;
    }
}

pub fn get_playlist(playlist_file: &Path) -> Playlist {
    unsafe {
        let _lock = CACHE_MUTEX.lock();
        
        if let Some((ref cached_playlist, cache_time)) = CACHED_PLAYLIST {
            if cache_time.elapsed().as_secs() < 30 {
                return cached_playlist.clone();
            }
        }
        
        let playlist = read_playlist_from_file(playlist_file);
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
    
    Playlist::default()
}

pub fn save_playlist(playlist: &Playlist, playlist_file: &Path) {
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
    
    if playlist.tracks.is_empty() {
        return None;
    }
    
    let current_index = if playlist.current_track >= playlist.tracks.len() {
        0
    } else {
        playlist.current_track
    };
    
    let current_track = playlist.tracks.get(current_index)?;
    let track_path = music_folder.join(&current_track.path);
    
    if !track_path.exists() {
        log::warn!("Current track file not found: {}", track_path.display());
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
            log::info!("Adding new track: {} by {}", mp3.title, mp3.artist);
            playlist.tracks.push(mp3);
            added_count += 1;
        }
    }
    
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
    
    if !playlist.tracks.is_empty() && playlist.current_track >= playlist.tracks.len() {
        playlist.current_track = 0;
        log::info!("Reset current track index to 0");
    }
    
    save_playlist(&playlist, playlist_file);
    playlist
}

pub fn verify_track_durations(playlist_file: &Path, music_folder: &Path) {
    let playlist = get_playlist(playlist_file);
    
    println!("============ TRACK DURATION VERIFICATION ============");
    println!("Current track index: {}", playlist.current_track);
    println!("Total tracks: {}", playlist.tracks.len());
    
    for (i, track) in playlist.tracks.iter().enumerate() {
        let file_path = music_folder.join(&track.path);
        let file_exists = file_path.exists();
        
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

pub fn rescan_and_update_durations(playlist_file: &Path, music_folder: &Path) {
    println!("Rescanning music folder and updating track durations...");
    
    let mut playlist = get_playlist(playlist_file);
    let mut updated_count = 0;
    
    for track in &mut playlist.tracks {
        let file_path = music_folder.join(&track.path);
        
        if !file_path.exists() {
            continue;
        }
        
        let old_duration = track.duration;
        
        match mp3_duration::from_path(&file_path) {
            Ok(d) => {
                let new_duration = d.as_secs();
                
                if new_duration != old_duration && new_duration > 0 {
                    println!("Updating duration for \"{}\" by \"{}\": {} -> {} seconds", 
                             track.title, track.artist, old_duration, new_duration);
                    track.duration = new_duration;
                    updated_count += 1;
                } else if new_duration == 0 {
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
            },
            Err(e) => {
                println!("Error getting duration for {}: {}", file_path.display(), e);
                
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
    save_playlist(&playlist, playlist_file);
    println!("Playlist updated and saved");
}

// MINIMAL track monitor - NO TRACK CONTROL, just status logging and maintenance
pub fn track_switcher(stream_manager: Arc<StreamManager>) {
    println!("Minimal monitor started - NO track control, just status logging");
    
    let mut last_log_time = Instant::now();
    let mut last_cleanup_time = Instant::now();
    let mut last_file_scan = Instant::now();
    
    loop {
        thread::sleep(Duration::from_secs(10)); // Check every 10 seconds
        
        // Clean up stale connections periodically
        if last_cleanup_time.elapsed().as_secs() >= CLEANUP_INTERVAL_SECS {
            stream_manager.cleanup_stale_connections();
            last_cleanup_time = Instant::now();
        }
        
        // Log status periodically
        if last_log_time.elapsed().as_secs() >= STATUS_LOG_INTERVAL_SECS {
            let active_listeners = stream_manager.get_active_listeners();
            let is_streaming = stream_manager.is_streaming();
            let track_state = stream_manager.get_track_state();
            
            if let Some(track_info) = &track_state.track_info {
                if let Ok(track_data) = serde_json::from_str::<serde_json::Value>(track_info) {
                    if let (Some(title), Some(artist)) = (
                        track_data.get("title").and_then(|t| t.as_str()),
                        track_data.get("artist").and_then(|a| a.as_str())
                    ) {
                        log::info!("STATUS: \"{}\" by {} at {}s/{}s, {} listeners", 
                                  title, artist, track_state.position_seconds, 
                                  track_state.duration, active_listeners);
                    }
                }
            }
            
            log::info!("Streaming: {}, Listeners: {}", is_streaming, active_listeners);
            last_log_time = Instant::now();
        }
        
        // Check for new files periodically
        if last_file_scan.elapsed().as_secs() >= FILE_SCAN_INTERVAL_SECS {
            let playlist = get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                log::info!("No tracks in playlist, scanning for new files...");
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
    
    let track_path = music_folder.join(&next_track.path);
    if !track_path.exists() {
        return None;
    }
    
    Some(next_track.clone())
}
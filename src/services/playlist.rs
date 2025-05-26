use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::Arc;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

// Constants to fine-tune track switching behavior
const MIN_TRACK_PLAYBACK_TIME: u64 = 5; // Minimum seconds a track must play before switching
const TRACK_SWITCH_DELAY_MS: u64 = 200; // Delay between track switch operations
const MAX_WAIT_BEFORE_SWITCH: u64 = 5; // Maximum seconds to wait before switching tracks
const HEALTH_CHECK_INTERVAL_SECS: u64 = 3; // Interval for checking stream health
const TRACK_SWITCH_TIMEOUT_SECS: u64 = 30; // Maximum time allowed for track switching

pub fn get_playlist(playlist_file: &Path) -> Playlist {
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
    
    // Verify file exists
    let track_path = music_folder.join(&current_track.path);
    if !track_path.exists() {
        // Try to advance if current file doesn't exist
        return advance_track(playlist_file, music_folder);
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
    playlist.current_track = (playlist.current_track + 1) % playlist.tracks.len();
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
    for mp3 in mp3_files {
        if !existing_paths.contains(&mp3.path) {
            playlist.tracks.push(mp3);
        }
    }
    
    // Remove tracks that no longer exist
    playlist.tracks.retain(|track| {
        music_folder.join(&track.path).exists()
    });
    
    // Make sure current_track is valid
    if !playlist.tracks.is_empty() && playlist.current_track >= playlist.tracks.len() {
        playlist.current_track = 0;
    }
    
    save_playlist(&playlist, playlist_file);
    playlist
}

// Verify track durations and report any issues
pub fn verify_track_durations(playlist_file: &Path, music_folder: &Path) {
    let playlist = get_playlist(playlist_file);
    
    println!("============ TRACK DURATION VERIFICATION ============");
    println!("Current track index: {}", playlist.current_track);
    println!("Total tracks: {}", playlist.tracks.len());
    
    for (i, track) in playlist.tracks.iter().enumerate() {
        let file_path = music_folder.join(&track.path);
        let file_exists = file_path.exists();
        
        // Attempt to verify duration directly from file
        let actual_duration = if file_exists {
            match mp3_duration::from_path(&file_path) {
                Ok(duration) => duration.as_secs(),
                Err(_) => 0
            }
        } else {
            0
        };
        
        println!("Track {}: \"{}\" by \"{}\"", i, track.title, track.artist);
        println!("  → Stored duration: {} seconds", track.duration);
        println!("  → Actual duration: {} seconds", actual_duration);
        println!("  → File exists: {}", file_exists);
        println!("  → Path: {}", file_path.display());
        
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

// Rescan and fix track durations
pub fn rescan_and_update_durations(playlist_file: &Path, music_folder: &Path) {
    println!("Rescanning music folder and updating track durations...");
    
    // Get current playlist
    let mut playlist = get_playlist(playlist_file);
    
    // Keep track of which tracks were updated
    let mut updated_count = 0;
    
    // Iterate through all tracks and update their durations
    for track in &mut playlist.tracks {
        let file_path = music_folder.join(&track.path);
        
        // Skip if file doesn't exist
        if !file_path.exists() {
            println!("File not found, skipping: {}", file_path.display());
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
        }
    }
    
    println!("Updated {} track durations", updated_count);
    
    // Save updated playlist
    save_playlist(&playlist, playlist_file);
    println!("Playlist updated and saved");
}

pub fn track_switcher(stream_manager: Arc<StreamManager>) {
    println!("Track monitor thread started - broadcast thread handles all transitions");
    
    let mut last_log_time = Instant::now();
    let mut last_cleanup_time = Instant::now();
    
    loop {
        thread::sleep(Duration::from_secs(1));
        
        // Clean up stale connections every 10 seconds
        if last_cleanup_time.elapsed().as_secs() >= 10 {
            stream_manager.cleanup_stale_connections();
            last_cleanup_time = Instant::now();
        }
        
        // Just monitor and log status
        if last_log_time.elapsed().as_secs() >= 10 {
            let active_listeners = stream_manager.get_active_listeners();
            let is_streaming = stream_manager.is_streaming();
            let playback_position = stream_manager.get_playback_position();
            
            if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                println!("MONITOR: Playing \"{}\" at {}s of {}s, listeners={}, streaming={}", 
                       track.title, playback_position, track.duration, active_listeners, is_streaming);
            }
            
            last_log_time = Instant::now();
        }
        
        // Check if we should scan for new files periodically
        if last_log_time.elapsed().as_secs() >= 300 { // Every 5 minutes
            let playlist = get_playlist(&crate::config::PLAYLIST_FILE);
            
            if playlist.tracks.is_empty() {
                println!("MONITOR: No tracks in playlist, scanning for new files...");
                scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
            }
        }
    }
}
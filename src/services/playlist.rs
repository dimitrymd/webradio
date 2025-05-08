use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::thread;
use std::time::Duration;

use crate::models::playlist::{Track, Playlist};
use crate::services::streamer::StreamManager;
use crate::utils::mp3_scanner;

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
    
    let mut file = File::create(playlist_file).unwrap_or_else(|e| {
        log::error!("Error creating playlist file: {}", e);
        panic!("Failed to create playlist file");
    });
    
    file.write_all(json.as_bytes()).unwrap_or_else(|e| {
        log::error!("Error writing playlist file: {}", e);
    });
}

pub fn get_current_track(playlist_file: &Path, music_folder: &Path) -> Option<Track> {
    let playlist = get_playlist(playlist_file);
    
    // Return None if no tracks
    if playlist.tracks.is_empty() {
        return None;
    }
    
    // Get current track
    let current_track = playlist.tracks.get(playlist.current_track)?;
    
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

pub fn track_switcher(stream_manager: StreamManager) {
    let mut prev_track_path = String::new();
    let mut playback_start_time = std::time::Instant::now();
    
    println!("Track switcher thread started - automatic broadcast mode");
    
    // We'll skip the initial track start here since it's already started in main.rs
    // Just get the current track for tracking
    if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        prev_track_path = track.path.clone();
        playback_start_time = std::time::Instant::now();
        println!("Track switcher monitoring initial track: \"{}\" by \"{}\" (duration: {} seconds)", 
                track.title, track.artist, track.duration);
    }
    
    let mut last_log_time = std::time::Instant::now();
    let mut force_next_warning_shown = false;
    
    loop {
        // Get current track path
        let track = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
        
        // Skip if no tracks available
        if track.is_none() {
            println!("WARNING: No tracks available for playback");
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        
        let current_track = track.unwrap();
        
        // Check if track has changed (this shouldn't normally happen except through the track switcher itself)
        if current_track.path != prev_track_path {
            println!("Track change detected: \"{}\" by \"{}\" (duration: {} seconds)", 
                   current_track.title, current_track.artist, current_track.duration);
            
            // Start streaming the current track if it's not already streaming
            stream_manager.start_streaming(&current_track.path);
            
            // Remember current track and reset timer
            prev_track_path = current_track.path.clone();
            playback_start_time = std::time::Instant::now();
            last_log_time = std::time::Instant::now();
            force_next_warning_shown = false;
            
            println!("Started playing new track at {}", 
                   chrono::Local::now().format("%H:%M:%S%.3f"));
        }
        
        // Calculate how long we've been playing this track
        let elapsed = playback_start_time.elapsed().as_secs();
        
        // Log playback progress every 5 seconds
        if last_log_time.elapsed().as_secs() >= 5 {
            // Check streaming status
            let is_streaming = stream_manager.is_streaming();
            
            println!("PLAYBACK STATUS: Playing \"{}\" for {} seconds (of {} total), streaming={}", 
                   current_track.title, elapsed, current_track.duration, is_streaming);
            
            // If track has a duration set and we've exceeded it by 5 seconds
            if current_track.duration > 0 && elapsed > current_track.duration + 5 {
                // Show warning once
                if !force_next_warning_shown {
                    println!("WARNING: Track has been playing for longer than its duration! {} seconds played, {} seconds expected", 
                           elapsed, current_track.duration);
                    force_next_warning_shown = true;
                }
                
                // CRITICAL FIX: If we've exceeded duration by more than 10 seconds, force next track
                if elapsed > current_track.duration + 10 {
                    println!("CRITICAL: Forcing next track after playing for {} seconds (duration is {} seconds)", 
                            elapsed, current_track.duration);
                    
                    // Force stop the current stream
                    println!("Forcing stream to stop");
                    stream_manager.force_stop_streaming();
                    
                    // Force advance to next track
                    let next_track = advance_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
                    
                    if let Some(track) = next_track {
                        println!("Forced next track: \"{}\" by \"{}\" (duration: {} seconds)", 
                               track.title, track.artist, track.duration);
                        
                        // Start streaming the next track
                        stream_manager.start_streaming(&track.path);
                        prev_track_path = track.path.clone();
                        playback_start_time = std::time::Instant::now();
                        force_next_warning_shown = false;
                    }
                    
                    last_log_time = std::time::Instant::now();
                    continue;
                }
            }
            
            last_log_time = std::time::Instant::now();
        }
        
        // Check if streaming has stopped (track ended naturally)
        let is_streaming = stream_manager.is_streaming();
        
        if !is_streaming {
            println!("Stream ended for track: \"{}\" after {} seconds of playback. Advancing to next track.", 
                   current_track.title, elapsed);
            
            // Advance to next track
            let next_track = advance_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
            
            if let Some(track) = next_track {
                println!("Starting next track: \"{}\" by \"{}\" (duration: {} seconds)", 
                       track.title, track.artist, track.duration);
                       
                // Start streaming the next track
                stream_manager.start_streaming(&track.path);
                prev_track_path = track.path.clone();
                playback_start_time = std::time::Instant::now();
                last_log_time = std::time::Instant::now();
                force_next_warning_shown = false;
            } else {
                println!("WARNING: Failed to get next track");
            }
            
            // Sleep briefly to avoid tight loops
            thread::sleep(Duration::from_millis(200));
            continue;
        }
        
        // Sleep to avoid tight loop
        thread::sleep(Duration::from_secs(1));
    }
}
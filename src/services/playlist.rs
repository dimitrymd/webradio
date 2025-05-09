use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

// Track switcher that manages broadcast playback
pub fn track_switcher(stream_manager: StreamManager) {
    // First, verify track durations to identify potential issues
    verify_track_durations(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
    
    let mut prev_track_path = String::new();
    let mut playback_start_time = std::time::Instant::now();
    let mut last_health_check = std::time::Instant::now();
    
    // Track if we're currently switching tracks (to avoid race conditions)
    let is_switching = Arc::new(AtomicBool::new(false));
    
    println!("Track switcher thread started - broadcast mode");
    
    // We'll skip the initial track start here since it's already started in main.rs
    // Just get the current track for tracking
    if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        prev_track_path = track.path.clone();
        playback_start_time = std::time::Instant::now();
        println!("Track switcher monitoring initial track: \"{}\" by \"{}\" (duration: {} seconds)", 
                track.title, track.artist, track.duration);
    }
    
    let mut last_log_time = std::time::Instant::now();
    let mut first_run = true;  // Flag for first iteration of the loop
    
    loop {
        // Small delay at the start of each iteration to avoid tight loop
        thread::sleep(Duration::from_millis(200));
        
        // Get current track path
        let track = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
        
        // Skip if no tracks available
        if track.is_none() {
            println!("WARNING: No tracks available for playback");
            thread::sleep(Duration::from_secs(1));
            
            // Try to scan music folder for new tracks
            println!("Scanning music folder for new tracks...");
            scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
            
            continue;
        }
        
        let current_track = track.unwrap();
        
        // For the first run of the loop, just set up tracking without taking any action
        if first_run {
            prev_track_path = current_track.path.clone();
            playback_start_time = std::time::Instant::now();
            last_log_time = std::time::Instant::now();
            first_run = false;
            
            println!("Initial track monitoring set up: \"{}\" by \"{}\"", 
                   current_track.title, current_track.artist);
                   
            continue;
        }
        
        // Check if track has changed (this shouldn't normally happen except through the track switcher itself)
        if current_track.path != prev_track_path {
            // Set the switching flag to prevent concurrent track changes
            is_switching.store(true, Ordering::SeqCst);
            
            println!("Track change detected: \"{}\" by \"{}\" (duration: {} seconds)", 
                   current_track.title, current_track.artist, current_track.duration);
            
            // Reset track ended flag
            stream_manager.reset_track_ended_flag();
            
            // Start streaming (broadcasting) the current track
            stream_manager.start_streaming(&current_track.path);
            
            // Remember current track and reset timer
            prev_track_path = current_track.path.clone();
            playback_start_time = std::time::Instant::now();
            last_log_time = std::time::Instant::now();
            
            println!("Started broadcasting new track at {}", 
                   chrono::Local::now().format("%H:%M:%S%.3f"));
                   
            // Clear the switching flag
            is_switching.store(false, Ordering::SeqCst);
        }
        
        // Check stream health every 3 seconds
        if last_health_check.elapsed().as_secs() >= 3 {
            // Check if stream is stalled
            if stream_manager.is_stream_stalled() && !is_switching.load(Ordering::SeqCst) {
                println!("WARNING: Stream appears to be stalled. Restarting broadcast.");
                
                // Set the switching flag to prevent concurrent track changes
                is_switching.store(true, Ordering::SeqCst);
                
                // Force stop current stream
                stream_manager.force_stop_streaming();
                
                // Restart the same track (without advancing)
                if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                    println!("Restarting broadcast of track: \"{}\" by \"{}\"", 
                           track.title, track.artist);
                    
                    // Start streaming the track again
                    stream_manager.start_streaming(&track.path);
                    prev_track_path = track.path.clone();
                    playback_start_time = std::time::Instant::now();
                    last_log_time = std::time::Instant::now();
                }
                
                // Clear the switching flag
                is_switching.store(false, Ordering::SeqCst);
            }
            
            last_health_check = std::time::Instant::now();
        }
        
        // Calculate how long we've been playing this track
        let elapsed_time = playback_start_time.elapsed().as_secs();
        
        // Get current playback position from the stream manager
        let current_position = stream_manager.get_playback_position();
        
        // Log playback progress every 5 seconds
        if last_log_time.elapsed().as_secs() >= 5 {
            // Check streaming status
            let is_streaming = stream_manager.is_streaming();
            let track_ended = stream_manager.track_ended();
            let receiver_count = stream_manager.get_receiver_count();
            
            println!("BROADCAST STATUS: Playing \"{}\" for {} seconds (of {} total), streaming={}, track_ended={}, listeners={}", 
                   current_track.title, elapsed_time, current_track.duration, is_streaming, track_ended, receiver_count);
            
            last_log_time = std::time::Instant::now();
        }
        
        // CHECK 1: Ensure we've been playing for at least a minimum time before considering track switches
        if elapsed_time < 5 {
            continue;  // Skip the rest of the loop if we just started playing
        }
        
        // CHECK 2: Track has a defined duration and we've exceeded it by a margin
        let duration_check = current_track.duration > 10 && elapsed_time >= current_track.duration + 2;
        
        // CHECK 3: Stream has explicitly signaled track end and we're at least reasonably close to expected duration
        let end_flag_check = stream_manager.track_ended() && elapsed_time >= current_track.duration / 2;
        
        // CHECK 4: Stream is no longer active but was previously (and not due to a recent switch)
        let streaming_check = !stream_manager.is_streaming() && elapsed_time > current_track.duration / 2;
        
        if (duration_check || end_flag_check || streaming_check) && !is_switching.load(Ordering::SeqCst) {
            println!("Track complete: \"{}\" (duration check: {}, end flag: {}, streaming check: {}). Advancing to next track.", 
                   current_track.title, duration_check, end_flag_check, streaming_check);
            println!("Elapsed time: {}s, Track duration: {}s", elapsed_time, current_track.duration);
            
            // Set the switching flag to prevent concurrent track changes
            is_switching.store(true, Ordering::SeqCst);
            
            // Sleep a moment to ensure full track playback
            // This adds a safety margin to ensure we don't switch tracks too early
            if elapsed_time < current_track.duration {
                let wait_time = current_track.duration - elapsed_time + 1;
                if wait_time > 0 && wait_time < 30 { // Don't wait more than 30 seconds as a safeguard
                    println!("Waiting additional {} seconds before advancing track", wait_time);
                    thread::sleep(Duration::from_secs(wait_time));
                }
            }
            
            // Force stop the current stream if it's still running
            if stream_manager.is_streaming() {
                println!("Forcing stream to stop before switching tracks");
                stream_manager.force_stop_streaming();
            }
            
            // Sleep a moment to ensure cleanup is done
            thread::sleep(Duration::from_secs(1));
            
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
            } else {
                println!("WARNING: Failed to get next track");
            }
            
            // Clear the switching flag
            is_switching.store(false, Ordering::SeqCst);
            
            // Sleep briefly to avoid tight loops
            thread::sleep(Duration::from_millis(500));
        }
    }
}
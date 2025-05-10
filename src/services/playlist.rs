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

// Fixed track_switcher function with improved synchronization
pub fn track_switcher(stream_manager: StreamManager) {
    // First, verify track durations to identify potential issues
    verify_track_durations(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
    
    let mut prev_track_path = String::new();
    let mut playback_start_time = std::time::Instant::now();
    let mut last_health_check = std::time::Instant::now();
    let mut last_track_ended_time = Instant::now();
    
    // Track if we're currently switching tracks (to avoid race conditions)
    let is_switching = Arc::new(AtomicBool::new(false));
    
    println!("Track switcher thread started - broadcast mode with improved synchronization");
    
    // Get the current track for tracking
    if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
        prev_track_path = track.path.clone();
        playback_start_time = std::time::Instant::now();
        println!("Track switcher monitoring initial track: \"{}\" by \"{}\" (duration: {} seconds)", 
                track.title, track.artist, track.duration);
    }
    
    let mut last_log_time = std::time::Instant::now();
    let mut first_run = true;  // Flag for first iteration of the loop
    
    // Continue running until program exits
    loop {
        // Small delay at the start of each iteration to avoid tight loop
        thread::sleep(Duration::from_millis(100));
        
        // Quick check if server is shutting down
        if thread::panicking() {
            println!("Detected panic - track switcher shutting down");
            break;
        }
        
        // Get current track
        let track = match get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
            Some(track) => track,
            None => {
                println!("WARNING: No tracks available for playback");
                thread::sleep(Duration::from_secs(1));
                
                // Try to scan music folder for new tracks
                println!("Scanning music folder for new tracks...");
                scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
                
                continue;
            }
        };
        
        // Check if we've been stuck in track ended state for too long
        if stream_manager.track_ended() && 
           !is_switching.load(Ordering::SeqCst) &&
           last_track_ended_time.elapsed().as_secs() > TRACK_SWITCH_TIMEOUT_SECS {
            
            println!("WARNING: Stuck in track_ended state for too long, forcing track switch");
            
            // Try to initiate track switching with compare_exchange for thread safety
            if is_switching.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                println!("Forcing track switch after timeout");
                
                // Reset track ended time
                last_track_ended_time = Instant::now();
                
                // Force track switch
                match advance_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                    Some(next_track) => {
                        println!("Force starting next track: \"{}\"", next_track.title);
                        
                        // Clear the track ended flag BEFORE starting the new track
                        stream_manager.reset_track_ended_flag();
                        
                        // Start streaming the new track
                        stream_manager.start_streaming(&next_track.path);
                        
                        // Update tracking variables
                        prev_track_path = next_track.path.clone();
                        playback_start_time = std::time::Instant::now();
                        last_log_time = std::time::Instant::now();
                    },
                    None => {
                        println!("CRITICAL: No next track available during force switch");
                        // Try emergency recovery
                        scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
                        
                        if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                            println!("Emergency recovery successful");
                            stream_manager.reset_track_ended_flag();
                            stream_manager.start_streaming(&track.path);
                            prev_track_path = track.path.clone();
                            playback_start_time = std::time::Instant::now();
                            last_log_time = std::time::Instant::now();
                        } else {
                            println!("FATAL: Emergency recovery failed - no tracks available");
                        }
                    }
                }
                
                // Always clear the switching flag
                is_switching.store(false, Ordering::SeqCst);
            }
        }
        
        // For the first run of the loop, just set up tracking without taking any action
        if first_run {
            prev_track_path = track.path.clone();
            playback_start_time = std::time::Instant::now();
            last_log_time = std::time::Instant::now();
            first_run = false;
            
            println!("Initial track monitoring set up: \"{}\" by \"{}\"", 
                   track.title, track.artist);
                   
            continue;
        }
        
        // Check if track has changed (this shouldn't normally happen except through the track switcher itself)
        if track.path != prev_track_path {
            // Try to set the switching flag - if another thread is already switching, we'll skip
            if is_switching.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                println!("Track change detected: \"{}\" by \"{}\" (duration: {} seconds)", 
                       track.title, track.artist, track.duration);
                
                // Reset track ended flag
                stream_manager.reset_track_ended_flag();
                
                // Start streaming (broadcasting) the current track
                stream_manager.start_streaming(&track.path);
                
                // Remember current track and reset timer
                prev_track_path = track.path.clone();
                playback_start_time = std::time::Instant::now();
                last_log_time = std::time::Instant::now();
                
                println!("Started broadcasting new track at {}", 
                       chrono::Local::now().format("%H:%M:%S%.3f"));
                       
                // Clear the switching flag
                is_switching.store(false, Ordering::SeqCst);
            } else {
                println!("Track change detected but another thread is already handling switching");
            }
        }
        
        // Check stream health periodically
        if last_health_check.elapsed().as_secs() >= HEALTH_CHECK_INTERVAL_SECS {
            // Check if stream is stalled
            if stream_manager.is_stream_stalled() && 
               !is_switching.load(Ordering::SeqCst) &&
               is_switching.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                
                println!("WARNING: Stream appears to be stalled. Restarting broadcast.");
                
                // Force stop current stream
                stream_manager.force_stop_streaming();
                
                // Brief pause to allow cleanup
                thread::sleep(Duration::from_millis(200));
                
                // Restart the same track (without advancing)
                if let Some(track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                    println!("Restarting broadcast of track: \"{}\" by \"{}\"", 
                           track.title, track.artist);
                    
                    // Reset flags and start streaming
                    stream_manager.reset_track_ended_flag();
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
        
        // Log playback progress periodically
        if last_log_time.elapsed().as_secs() >= 5 {
            // Check streaming status - use fast atomic operations
            let is_streaming = stream_manager.is_streaming();
            let track_ended = stream_manager.track_ended();
            let active_listeners = stream_manager.get_active_listeners();
            
            println!("BROADCAST STATUS: Playing \"{}\" for {} seconds (of {} total), streaming={}, track_ended={}, listeners={}", 
                   track.title, elapsed_time, track.duration, is_streaming, track_ended, active_listeners);
            
            last_log_time = std::time::Instant::now();
        }
        
        // Ensure we've been playing for at least a minimum time before considering track switches
        if elapsed_time < MIN_TRACK_PLAYBACK_TIME {
            continue;
        }
        
        // Track ending conditions (using atomic flags for faster checks)
        let duration_check = track.duration > 10 && elapsed_time >= track.duration + 2;
        let end_flag_check = stream_manager.track_ended() && elapsed_time >= track.duration / 2;
        
        // Check if we need to switch tracks
        if (duration_check || end_flag_check) && 
            !is_switching.load(Ordering::SeqCst) &&
            is_switching.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            
            println!("Track switch starting for track: \"{}\"", track.title);
            
            // Track the start time for timeout detection
            let switch_start_time = Instant::now();
            last_track_ended_time = Instant::now();
            
            // Try to advance to the next track
            let next_track_result = advance_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
            
            // Prepare for track switch
            stream_manager.prepare_for_track_switch();
            
            // Small delay to ensure all clients receive the end-of-track signal
            thread::sleep(Duration::from_millis(TRACK_SWITCH_DELAY_MS));
            
            match next_track_result {
                Some(next_track) => {
                    println!("Successfully advanced to next track: \"{}\" by \"{}\"", 
                            next_track.title, next_track.artist);
                    
                    // Reset the track ended flag BEFORE starting new track
                    stream_manager.reset_track_ended_flag();
                    
                    // Start streaming the next track
                    stream_manager.start_streaming(&next_track.path);
                    
                    // Update tracking variables
                    prev_track_path = next_track.path.clone();
                    playback_start_time = std::time::Instant::now();
                    last_log_time = std::time::Instant::now();
                },
                None => {
                    println!("CRITICAL: Failed to get next track. Attempting recovery...");
                    
                    // Recovery attempt - scan music folder for new tracks
                    scan_music_folder(&crate::config::MUSIC_FOLDER, &crate::config::PLAYLIST_FILE);
                    
                    // Try again with the freshly scanned playlist
                    if let Some(recovered_track) = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER) {
                        println!("Recovery successful - starting track: \"{}\"", recovered_track.title);
                        
                        // Reset the track ended flag
                        stream_manager.reset_track_ended_flag();
                        stream_manager.start_streaming(&recovered_track.path);
                        
                        // Update tracking variables
                        prev_track_path = recovered_track.path.clone();
                        playback_start_time = std::time::Instant::now();
                        last_log_time = std::time::Instant::now();
                    } else {
                        println!("FATAL: No tracks available after recovery attempt");
                        // Force stream_manager into a valid state to avoid deadlock
                        stream_manager.force_stop_streaming();
                    }
                }
            }
            
            // CRITICAL: Always clear the switching flag, even if an error occurred
            is_switching.store(false, Ordering::SeqCst);
            
            // Check if switch took too long
            if switch_start_time.elapsed().as_secs() > TRACK_SWITCH_TIMEOUT_SECS {
                println!("WARNING: Track switch took longer than expected: {:?}", 
                       switch_start_time.elapsed());
            } else {
                println!("Track switch completed in {:?}", switch_start_time.elapsed());
            }
        }
    }
}
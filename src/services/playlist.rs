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

pub fn shuffle_playlist(playlist_file: &Path, _music_folder: &Path) -> Playlist {
    let mut playlist = get_playlist(playlist_file);
    
    // Keep track of current track
    let current = playlist.tracks.get(playlist.current_track).cloned();
    
    // Shuffle tracks
    let mut rng = thread_rng();
    playlist.tracks.shuffle(&mut rng);
    
    // Find current track in shuffled list
    if let Some(current_track) = current {
        for (i, track) in playlist.tracks.iter().enumerate() {
            if track.path == current_track.path {
                playlist.current_track = i;
                break;
            }
        }
    }
    
    save_playlist(&playlist, playlist_file);
    playlist
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
    
    loop {
        // Get current track path
        let track = get_current_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
        
        // Skip if no tracks available
        if track.is_none() {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        
        let current_track = track.unwrap();
        
        // Skip if track is the same as before
        if current_track.path == prev_track_path {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        
        // Start streaming the current track
        stream_manager.start_streaming(&current_track.path);
        
        // Remember current track
        prev_track_path = current_track.path.clone();
        
        // Wait for track to finish streaming
        while stream_manager.is_streaming() {
            thread::sleep(Duration::from_secs(1));
        }
        
        // Advance to next track
        advance_track(&crate::config::PLAYLIST_FILE, &crate::config::MUSIC_FOLDER);
    }
}
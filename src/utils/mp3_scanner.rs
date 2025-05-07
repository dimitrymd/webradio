use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};
use id3::{Tag, TagLike};
use mp3_duration;
use log::{info, error};

use crate::models::playlist::Track;

pub fn scan_directory(dir: &Path) -> Vec<Track> {
    let mut tracks = Vec::new();
    
    if !dir.exists() {
        // Create the directory if it doesn't exist
        fs::create_dir_all(dir).unwrap_or_else(|e| {
            error!("Failed to create directory: {}", e);
        });
        return tracks;
    }
    
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                
                if path.is_dir() {
                    // Recursive scan
                    let mut sub_tracks = scan_directory(&path);
                    tracks.append(&mut sub_tracks);
                } else if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "mp3" {
                        if let Some(track) = process_mp3_file(&path, dir) {
                            tracks.push(track);
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Error reading directory {}: {}", dir.display(), e);
        }
    }
    
    tracks
}

fn process_mp3_file(file_path: &Path, base_dir: &Path) -> Option<Track> {
    // Get relative path
    let rel_path = file_path.strip_prefix(base_dir).ok()?;
    let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");
    
    // Extract metadata
    let (title, artist, album, duration) = extract_metadata(file_path);
    
    // Create track
    Some(Track {
        path: rel_path_str.to_string(),
        title,
        artist,
        album,
        duration,
    })
}

fn extract_metadata(file_path: &Path) -> (String, String, String, u64) {
    // Default values
    let file_name = file_path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    
    let mut title = file_name.clone();
    let mut artist = "Unknown".to_string();
    let mut album = "Unknown".to_string();
    let mut duration = 0;
    
    // Try to extract ID3 tags
    match Tag::read_from_path(file_path) {
        Ok(tag) => {
            if let Some(tag_title) = tag.title() {
                title = tag_title.to_string();
            }
            
            if let Some(tag_artist) = tag.artist() {
                artist = tag_artist.to_string();
            }
            
            if let Some(tag_album) = tag.album() {
                album = tag_album.to_string();
            }
        }
        Err(e) => {
            info!("Could not read ID3 tags from {}: {}", file_path.display(), e);
        }
    }
    
    // Try to get duration
    match mp3_duration::from_path(file_path) {
        Ok(d) => {
            duration = d.as_secs();
        }
        Err(e) => {
            info!("Could not get duration for {}: {}", file_path.display(), e);
        }
    }
    
    (title, artist, album, duration)
}
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, warn};
use id3::TagLike;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub tracks: Vec<Track>,
    #[serde(default)]
    current_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<u64>,
    pub bitrate: Option<u64>,
}

impl Playlist {
    pub async fn load_or_scan(music_dir: &Path) -> Result<Self> {
        let playlist_path = music_dir.join("playlist.json");
        
        // Try to load existing playlist
        if playlist_path.exists() {
            match Self::load(&playlist_path).await {
                Ok(playlist) => {
                    info!("Loaded playlist with {} tracks", playlist.tracks.len());
                    return Ok(playlist);
                }
                Err(e) => {
                    warn!("Failed to load playlist: {}", e);
                }
            }
        }
        
        // Scan for MP3 files
        info!("Scanning {} for MP3 files", music_dir.display());
        let playlist = Self::scan_directory(music_dir).await?;
        
        info!("Found {} MP3 files", playlist.tracks.len());
        
        // Log the tracks found
        for (i, track) in playlist.tracks.iter().enumerate() {
            info!("  [{}] {} - {} ({})", i, track.artist, track.title, track.path.display());
        }
        
        // Save for next time
        if let Err(e) = playlist.save(&playlist_path).await {
            warn!("Failed to save playlist: {}", e);
        }
        
        Ok(playlist)
    }
    
    async fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path).await?;
        let playlist = serde_json::from_str(&data)?;
        Ok(playlist)
    }
    
    async fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data).await?;
        Ok(())
    }
    
    async fn scan_directory(dir: &Path) -> Result<Self> {
        use std::pin::Pin;
        use std::future::Future;
        
        fn scan_directory_inner(
            dir: PathBuf,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Track>>> + Send>> {
            Box::pin(async move {
                let mut tracks = Vec::new();
                let mut entries = fs::read_dir(&dir).await?;
                
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    
                    if path.is_dir() {
                        // Recursively scan subdirectories
                        match scan_directory_inner(path).await {
                            Ok(mut subtracks) => tracks.append(&mut subtracks),
                            Err(e) => warn!("Failed to scan subdirectory: {}", e),
                        }
                    } else if path.extension().and_then(|s| s.to_str()) == Some("mp3") {
                        if let Some(track) = create_track_from_file(&path, &dir).await {
                            tracks.push(track);
                        }
                    }
                }
                
                Ok(tracks)
            })
        }
        
        async fn create_track_from_file(path: &Path, base_dir: &Path) -> Option<Track> {
            let relative_path = path.strip_prefix(base_dir).ok()?;
            
            // Extract metadata if possible
            let (title, artist, album) = match id3::Tag::read_from_path(path) {
                Ok(tag) => (
                    tag.title().unwrap_or("Unknown").to_string(),
                    tag.artist().unwrap_or("Unknown").to_string(),
                    tag.album().unwrap_or("Unknown").to_string(),
                ),
                Err(_) => {
                    let title = path.file_stem()?.to_string_lossy().to_string();
                    ("Unknown".to_string(), "Unknown".to_string(), title)
                }
            };
            
            // Get actual bitrate from MP3 file
            let bitrate = get_mp3_bitrate(path).await;
            
            // Get duration if possible
            let duration = get_mp3_duration(path).await;
            
            info!("Track: {} - Bitrate: {}kbps, Duration: {}s", 
                relative_path.display(), 
                bitrate.unwrap_or(0) / 1000,
                duration.unwrap_or(0)
            );
            
            Some(Track {
                path: relative_path.to_path_buf(),
                title,
                artist,
                album,
                duration,
                bitrate,
            })
        }
        
        let mut tracks = scan_directory_inner(dir.to_path_buf()).await?;
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        
        Ok(Playlist {
            tracks,
            current_index: 0,
        })
    }
    
    pub fn get_next_track(&mut self) -> Option<Track> {
        if self.tracks.is_empty() {
            return None;
        }
        
        let track = self.tracks[self.current_index].clone();
        self.current_index = (self.current_index + 1) % self.tracks.len();
        
        Some(track)
    }
}

// Helper function to get MP3 bitrate
async fn get_mp3_bitrate(path: &Path) -> Option<u64> {
    use tokio::io::AsyncReadExt;
    
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut buffer = vec![0u8; 4096]; // Read first 4KB
    let bytes_read = file.read(&mut buffer).await.ok()?;
    buffer.truncate(bytes_read);
    
    // Skip ID3v2 tag if present
    let mut offset = 0;
    if buffer.len() > 10 && &buffer[..3] == b"ID3" {
        let size = ((buffer[6] as usize & 0x7F) << 21)
            | ((buffer[7] as usize & 0x7F) << 14)
            | ((buffer[8] as usize & 0x7F) << 7)
            | (buffer[9] as usize & 0x7F);
        offset = 10 + size;
    }
    
    // Find first MP3 frame
    while offset + 4 <= buffer.len() {
        if buffer[offset] == 0xFF && (buffer[offset + 1] & 0xE0) == 0xE0 {
            // Parse MP3 header
            let header = ((buffer[offset] as u32) << 24)
                | ((buffer[offset + 1] as u32) << 16)
                | ((buffer[offset + 2] as u32) << 8)
                | (buffer[offset + 3] as u32);
            
            // Extract bitrate
            let version = (header >> 19) & 3;
            let layer = (header >> 17) & 3;
            let bitrate_index = (header >> 12) & 0xF;
            
            if version != 1 && layer == 1 && bitrate_index > 0 && bitrate_index < 15 {
                // MPEG1 Layer III (MP3) bitrates
                let bitrates = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
                return Some(bitrates[bitrate_index as usize] as u64 * 1000);
            }
        }
        offset += 1;
    }
    
    None
}

// Helper function to get MP3 duration
async fn get_mp3_duration(path: &Path) -> Option<u64> {
    // For now, we'll estimate based on file size and bitrate
    let metadata = tokio::fs::metadata(path).await.ok()?;
    let file_size = metadata.len();
    let bitrate = get_mp3_bitrate(path).await?;
    
    // Duration in seconds = (file_size * 8) / bitrate
    Some((file_size * 8) / bitrate)
}
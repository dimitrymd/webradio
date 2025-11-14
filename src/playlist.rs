use std::path::{Path, PathBuf};
use std::fs::File;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, warn};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::formats::FormatOptions;

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

            // Use symphonia to extract all metadata efficiently in one pass
            let (title, artist, album, duration, bitrate) = match extract_metadata_with_symphonia(path) {
                Some(metadata) => metadata,
                None => {
                    // Fallback: use filename as title
                    let title = path.file_stem()?.to_string_lossy().to_string();
                    (title, "Unknown".to_string(), "Unknown".to_string(), None, None)
                }
            };

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

// Extract all metadata efficiently using symphonia in one pass
// Returns: (title, artist, album, duration_secs, bitrate_bps)
fn extract_metadata_with_symphonia(path: &Path) -> Option<(String, String, String, Option<u64>, Option<u64>)> {
    // Get file size for bitrate calculation
    let file_size = std::fs::metadata(path).ok()?.len();

    // Open the file
    let file = File::open(path).ok()?;
    let media_source = MediaSourceStream::new(Box::new(file), Default::default());

    // Create a hint to help the probe guess the format
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the media source
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, media_source, &format_opts, &metadata_opts)
        .ok()?;

    let mut format = probed.format;

    // Extract metadata from tags
    let mut title = String::from("Unknown");
    let mut artist = String::from("Unknown");
    let mut album = String::from("Unknown");

    // Check for metadata in the format reader
    if let Some(metadata_rev) = format.metadata().current() {
        for tag in metadata_rev.tags() {
            match tag.std_key {
                Some(symphonia::core::meta::StandardTagKey::TrackTitle) => {
                    title = tag.value.to_string();
                }
                Some(symphonia::core::meta::StandardTagKey::Artist) => {
                    artist = tag.value.to_string();
                }
                Some(symphonia::core::meta::StandardTagKey::Album) => {
                    album = tag.value.to_string();
                }
                _ => {}
            }
        }
    }

    // Get the default audio track
    let track = format.default_track()?;

    // Extract duration
    let duration = if let Some(time_base) = track.codec_params.time_base {
        if let Some(n_frames) = track.codec_params.n_frames {
            let seconds = time_base.calc_time(n_frames).seconds;
            Some(seconds)
        } else {
            None
        }
    } else {
        None
    };

    // Calculate bitrate from file size and duration
    // Symphonia doesn't always provide bit_rate in CodecParameters for all formats
    // This approach gives accurate average bitrate for the entire file
    let bitrate = if let Some(dur) = duration {
        if dur > 0 {
            Some((file_size * 8) / dur)
        } else {
            None
        }
    } else {
        None
    };

    Some((title, artist, album, duration, bitrate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_creation() {
        let track = Track {
            path: PathBuf::from("test.mp3"),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            album: "Test Album".to_string(),
            duration: Some(180),
            bitrate: Some(192000),
        };

        assert_eq!(track.title, "Test Song");
        assert_eq!(track.artist, "Test Artist");
        assert_eq!(track.album, "Test Album");
        assert_eq!(track.duration, Some(180));
        assert_eq!(track.bitrate, Some(192000));
    }

    #[test]
    fn test_playlist_get_next_track() {
        let mut playlist = Playlist {
            tracks: vec![
                Track {
                    path: PathBuf::from("track1.mp3"),
                    title: "Song 1".to_string(),
                    artist: "Artist 1".to_string(),
                    album: "Album 1".to_string(),
                    duration: None,
                    bitrate: None,
                },
                Track {
                    path: PathBuf::from("track2.mp3"),
                    title: "Song 2".to_string(),
                    artist: "Artist 2".to_string(),
                    album: "Album 2".to_string(),
                    duration: None,
                    bitrate: None,
                },
                Track {
                    path: PathBuf::from("track3.mp3"),
                    title: "Song 3".to_string(),
                    artist: "Artist 3".to_string(),
                    album: "Album 3".to_string(),
                    duration: None,
                    bitrate: None,
                },
            ],
            current_index: 0,
        };

        // Get first track
        let track = playlist.get_next_track().unwrap();
        assert_eq!(track.title, "Song 1");
        assert_eq!(playlist.current_index, 1);

        // Get second track
        let track = playlist.get_next_track().unwrap();
        assert_eq!(track.title, "Song 2");
        assert_eq!(playlist.current_index, 2);

        // Get third track
        let track = playlist.get_next_track().unwrap();
        assert_eq!(track.title, "Song 3");
        assert_eq!(playlist.current_index, 0); // Should wrap around

        // Verify wrapping works
        let track = playlist.get_next_track().unwrap();
        assert_eq!(track.title, "Song 1");
        assert_eq!(playlist.current_index, 1);
    }

    #[test]
    fn test_playlist_empty() {
        let mut playlist = Playlist {
            tracks: vec![],
            current_index: 0,
        };

        assert!(playlist.get_next_track().is_none());
    }

    #[test]
    fn test_playlist_single_track() {
        let mut playlist = Playlist {
            tracks: vec![
                Track {
                    path: PathBuf::from("only.mp3"),
                    title: "Only Song".to_string(),
                    artist: "Only Artist".to_string(),
                    album: "Only Album".to_string(),
                    duration: Some(200),
                    bitrate: Some(128000),
                },
            ],
            current_index: 0,
        };

        // Should keep returning the same track and index should wrap
        for _ in 0..5 {
            let track = playlist.get_next_track().unwrap();
            assert_eq!(track.title, "Only Song");
            assert_eq!(playlist.current_index, 0);
        }
    }

    #[test]
    fn test_playlist_serialization() {
        let playlist = Playlist {
            tracks: vec![
                Track {
                    path: PathBuf::from("test.mp3"),
                    title: "Test".to_string(),
                    artist: "Artist".to_string(),
                    album: "Album".to_string(),
                    duration: Some(180),
                    bitrate: Some(192000),
                },
            ],
            current_index: 0,
        };

        // Serialize to JSON
        let json = serde_json::to_string(&playlist).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"artist\":\"Artist\""));

        // Deserialize back
        let deserialized: Playlist = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tracks.len(), 1);
        assert_eq!(deserialized.tracks[0].title, "Test");
        assert_eq!(deserialized.current_index, 0);
    }

    #[test]
    fn test_track_serialization() {
        let track = Track {
            path: PathBuf::from("music/song.mp3"),
            title: "Amazing Song".to_string(),
            artist: "Great Artist".to_string(),
            album: "Wonderful Album".to_string(),
            duration: Some(240),
            bitrate: Some(320000),
        };

        // Serialize
        let json = serde_json::to_string(&track).unwrap();

        // Deserialize
        let deserialized: Track = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "Amazing Song");
        assert_eq!(deserialized.artist, "Great Artist");
        assert_eq!(deserialized.album, "Wonderful Album");
        assert_eq!(deserialized.duration, Some(240));
        assert_eq!(deserialized.bitrate, Some(320000));
    }

}
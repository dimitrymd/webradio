use serde::{Serialize, Deserialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(default)]
    pub current_track: usize,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            current_track: 0,
            tracks: Vec::new(),
        }
    }
}
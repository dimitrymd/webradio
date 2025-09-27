use std::path::PathBuf;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub music_dir: PathBuf,
    pub chunk_size: usize,
    pub buffer_size: usize,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8000),
            music_dir: std::env::var("MUSIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("music")),
            chunk_size: 16384,  // 16KB
            buffer_size: 128,   // Number of chunks in broadcast buffer
        }
    }
}
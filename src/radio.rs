use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
    sync::{broadcast, RwLock},
    time::{interval, sleep},
};
use tokio_stream::Stream;
use axum::response::sse::Event;
use bytes::Bytes;
use dashmap::DashMap;
use arc_swap::ArcSwap;
use tracing::{info, warn, error};

use crate::{
    error::Result,
    playlist::{Playlist, Track},
    config::Config,
};

const CHUNK_SIZE: usize = 16384; // 16KB chunks
const BUFFER_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct RadioStation {
    config: Config,
    playlist: Arc<RwLock<Playlist>>,
    current_track: Arc<ArcSwap<Option<Track>>>,
    
    // Broadcasting
    broadcast_tx: broadcast::Sender<Bytes>,
    is_broadcasting: Arc<AtomicBool>,
    
    // Statistics
    listeners: Arc<DashMap<String, ListenerInfo>>,
    total_bytes_sent: Arc<AtomicU64>,
    current_position: Arc<AtomicU64>,
    start_time: Instant,
    
    // Control
    shutdown_tx: broadcast::Sender<()>,
}

#[derive(Debug)]
struct ListenerInfo {
    connected_at: Instant,
    bytes_received: u64,
}

impl RadioStation {
    pub async fn new(config: Config) -> Result<Self> {
        // Load playlist
        let playlist = Playlist::load_or_scan(&config.music_dir).await?;
        info!("Loaded {} tracks", playlist.tracks.len());
        
        // Create broadcast channel with reasonable capacity
        let (broadcast_tx, _) = broadcast::channel(128);
        let (shutdown_tx, _) = broadcast::channel(1);
        
        Ok(Self {
            config,
            playlist: Arc::new(RwLock::new(playlist)),
            current_track: Arc::new(ArcSwap::from_pointee(None)),
            broadcast_tx,
            is_broadcasting: Arc::new(AtomicBool::new(false)),
            listeners: Arc::new(DashMap::new()),
            total_bytes_sent: Arc::new(AtomicU64::new(0)),
            current_position: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            shutdown_tx,
        })
    }
    
    pub fn start_broadcast(&self) {
        if self.is_broadcasting.swap(true, Ordering::Relaxed) {
            warn!("Broadcast already running");
            return;
        }
        
        info!("Starting radio broadcast...");
        
        let station = self.clone();
        tokio::spawn(async move {
            if let Err(e) = station.broadcast_loop().await {
                error!("Broadcast loop error: {}", e);
            }
            // Ensure the flag is cleared if broadcast loop exits
            station.is_broadcasting.store(false, Ordering::Relaxed);
        });
    }
    
    pub async fn stop_broadcast(&self) {
        info!("Stopping broadcast...");
        self.is_broadcasting.store(false, Ordering::Relaxed);
        
        // Send shutdown signal
        if let Err(e) = self.shutdown_tx.send(()) {
            warn!("Failed to send shutdown signal: {}", e);
        }
        
        // Give some time for graceful shutdown
        sleep(Duration::from_millis(200)).await;
        
        // Force close all receivers
        drop(self.broadcast_tx.clone());
        
        info!("Radio broadcast stopped");
    }
    
    async fn broadcast_loop(&self) -> Result<()> {
        let mut shutdown = self.shutdown_tx.subscribe();
        
        info!("Broadcast loop started");
        
        loop {
            // Check if we should stop
            if !self.is_broadcasting.load(Ordering::Relaxed) {
                break;
            }
            
            // Get next track
            let track = {
                let mut playlist = self.playlist.write().await;
                playlist.get_next_track()
            };
            
            let Some(track) = track else {
                warn!("No tracks available in playlist");
                sleep(Duration::from_secs(5)).await;
                continue;
            };
            
            // Update current track
            self.current_track.store(Arc::new(Some(track.clone())));
            info!("Now playing: {} - {} ({})", track.artist, track.title, track.path.display());
            
            // Stream the track
            tokio::select! {
                result = self.stream_track(&track) => {
                    match result {
                        Ok(_) => info!("Track completed successfully"),
                        Err(e) => error!("Error streaming track: {}", e),
                    }
                }
                _ = shutdown.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
            }
            
            // Small gap between tracks
            sleep(Duration::from_millis(500)).await;
        }
        
        info!("Broadcast loop ended");
        Ok(())
    }
    
    async fn stream_track(&self, track: &Track) -> Result<()> {
        let path = self.config.music_dir.join(&track.path);
        
        info!("Streaming track: {} at {}kbps", path.display(), track.bitrate.unwrap_or(128000) / 1000);
        
        let file = File::open(&path).await?;
        let metadata = file.metadata().await?;
        let file_size = metadata.len();
        
        let mut reader = BufReader::new(file);
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut position = 0u64;
        
        // Skip ID3v2 tag if present
        let mut id3_buffer = vec![0u8; 10];
        if reader.read_exact(&mut id3_buffer).await.is_ok() {
            if &id3_buffer[..3] == b"ID3" {
                // Calculate ID3v2 tag size
                let size = ((id3_buffer[6] as u32 & 0x7F) << 21)
                    | ((id3_buffer[7] as u32 & 0x7F) << 14)
                    | ((id3_buffer[8] as u32 & 0x7F) << 7)
                    | (id3_buffer[9] as u32 & 0x7F);
                
                // Skip the ID3 tag
                let mut skip_buffer = vec![0u8; size as usize];
                reader.read_exact(&mut skip_buffer).await?;
                position = 10 + size as u64;
                
                info!("Skipped ID3v2 tag of {} bytes", 10 + size);
            } else {
                // Not an ID3 tag, rewind by sending the first 10 bytes
                if self.broadcast_tx.receiver_count() > 0 {
                    let _ = self.broadcast_tx.send(Bytes::copy_from_slice(&id3_buffer));
                    self.total_bytes_sent.fetch_add(10, Ordering::Relaxed);
                }
                position = 10;
            }
        }
        
        // Use actual bitrate from track or detect it
        let bitrate = track.bitrate.unwrap_or_else(|| {
            warn!("No bitrate info for track, defaulting to 192kbps");
            192000
        });
        
        // Calculate timing based on actual bitrate
        let chunk_duration_ms = (CHUNK_SIZE as f64 * 8.0 * 1000.0) / bitrate as f64;
        let chunk_duration = Duration::from_micros((chunk_duration_ms * 1000.0) as u64);
        
        info!("Streaming at {}kbps, chunk every {:.2}ms", bitrate / 1000, chunk_duration_ms);
        
        let mut interval = interval(chunk_duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        
        // Send initial chunks immediately for faster start
        let initial_chunks = 3;
        for _ in 0..initial_chunks {
            if position >= file_size {
                break;
            }
            
            let bytes_read = reader.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            
            let chunk = Bytes::copy_from_slice(&buffer[..bytes_read]);
            position += bytes_read as u64;
            
            if self.broadcast_tx.receiver_count() > 0 {
                let _ = self.broadcast_tx.send(chunk);
                self.total_bytes_sent.fetch_add(bytes_read as u64, Ordering::Relaxed);
            }
        }
        
        // Continue streaming with proper timing
        while position < file_size {
            // Check if we should stop
            if !self.is_broadcasting.load(Ordering::Relaxed) {
                break;
            }
            
            // Wait for next chunk time
            interval.tick().await;
            
            // Read chunk
            let bytes_read = reader.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            
            let chunk = Bytes::copy_from_slice(&buffer[..bytes_read]);
            position += bytes_read as u64;
            
            // Update position
            self.current_position.store(position, Ordering::Relaxed);
            
            // Broadcast to all listeners
            if self.broadcast_tx.receiver_count() > 0 {
                if let Err(e) = self.broadcast_tx.send(chunk) {
                    warn!("Failed to broadcast chunk: {}", e);
                }
                self.total_bytes_sent.fetch_add(bytes_read as u64, Ordering::Relaxed);
            }
        }
        
        info!("Finished streaming track: {} (sent {} MB)", 
            track.title, 
            self.total_bytes_sent.load(Ordering::Relaxed) as f64 / 1_048_576.0
        );
        Ok(())
    }
    
    pub async fn create_audio_stream(&self) -> Result<impl Stream<Item = Result<Bytes>>> {
        let listener_id = uuid::Uuid::new_v4().to_string();
        let mut receiver = self.broadcast_tx.subscribe();
        
        // Register listener
        self.listeners.insert(listener_id.clone(), ListenerInfo {
            connected_at: Instant::now(),
            bytes_received: 0,
        });
        
        let listeners = self.listeners.clone();
        
        info!("New audio listener connected: {}", &listener_id[..8]);
        
        Ok(async_stream::stream! {
            // Don't send initial silence - it can confuse some players
            
            loop {
                match receiver.recv().await {
                    Ok(chunk) => {
                        // Update listener stats
                        if let Some(mut info) = listeners.get_mut(&listener_id) {
                            info.bytes_received += chunk.len() as u64;
                        }
                        yield Ok(chunk);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("Listener {} lagged by {} messages", listener_id, skipped);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Broadcast closed for listener {}", &listener_id[..8]);
                        break;
                    }
                }
            }
            
            // Cleanup on disconnect
            listeners.remove(&listener_id);
            info!("Audio listener disconnected: {}", &listener_id[..8]);
        })
    }
    
    pub fn create_event_stream(&self) -> impl Stream<Item = Result<Event>> {
        let station = self.clone();
        
        // Don't count SSE connections as listeners
        async_stream::stream! {
            let mut interval = interval(Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                let event = Event::default()
                    .event("now-playing")
                    .json_data(station.get_now_playing())
                    .unwrap();
                    
                yield Ok(event);
            }
        }
    }
    
    pub fn get_now_playing(&self) -> serde_json::Value {
        let current = self.current_track.load();
        
        match current.as_ref() {
            Some(track) => serde_json::json!({
                "title": track.title,
                "artist": track.artist,
                "album": track.album,
                "duration": track.duration,
                "bitrate": track.bitrate.unwrap_or(0) / 1000, // Show in kbps
                "position": self.current_position.load(Ordering::Relaxed),
                "listeners": self.listener_count(),
            }),
            None => serde_json::json!({
                "title": "No track playing",
                "listeners": self.listener_count(),
            }),
        }
    }
    
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
    
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
    
    pub fn get_playlist(&self) -> Result<Playlist> {
        // This is sync but should be fast
        let playlist = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.playlist.read().await.clone()
            })
        });
        Ok(playlist)
    }
    
    pub fn get_statistics(&self) -> serde_json::Value {
        let total_mb = self.total_bytes_sent.load(Ordering::Relaxed) as f64 / 1_048_576.0;
        let listeners: Vec<_> = self.listeners.iter()
            .map(|entry| {
                let (id, info) = entry.pair();
                serde_json::json!({
                    "id": &id[..8],
                    "connected_seconds": info.connected_at.elapsed().as_secs(),
                    "mb_received": info.bytes_received as f64 / 1_048_576.0,
                })
            })
            .collect();
        
        serde_json::json!({
            "uptime_seconds": self.uptime_seconds(),
            "total_mb_sent": total_mb,
            "current_listeners": self.listener_count(),
            "is_broadcasting": self.is_broadcasting.load(Ordering::Relaxed),
            "listeners": listeners,
        })
    }
    
    pub fn is_broadcasting(&self) -> bool {
        self.is_broadcasting.load(Ordering::Relaxed)
    }
    
    pub fn get_broadcast_receiver_count(&self) -> usize {
        self.broadcast_tx.receiver_count()
    }
}

impl Drop for RadioStation {
    fn drop(&mut self) {
        info!("RadioStation dropping, stopping broadcast");
        self.is_broadcasting.store(false, Ordering::Relaxed);
        let _ = self.shutdown_tx.send(());
    }
}
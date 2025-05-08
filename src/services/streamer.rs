use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, error, warn, debug};

#[derive(Clone)]
pub struct StreamManager {
    inner: Arc<Mutex<StreamManagerInner>>,
}

struct StreamManagerInner {
    music_folder: PathBuf,
    chunk_size: usize,
    buffer_size: usize,
    cache_time: u64,
    
    // Track currently being streamed
    current_track_path: Option<String>,
    
    // Buffer for audio chunks
    buffer: VecDeque<Vec<u8>>,
    
    // Time when each chunk was added
    chunk_times: VecDeque<Instant>,
    
    // Active listener count
    active_listeners: usize,
    
    // Stream state
    streaming: bool,
    stream_thread: Option<JoinHandle<()>>,
}

impl StreamManager {
    pub fn new(music_folder: &Path, chunk_size: usize, buffer_size: usize, cache_time: u64) -> Self {
        info!("Initializing StreamManager with music_folder: {}, chunk_size: {}, buffer_size: {}, cache_time: {}",
            music_folder.display(), chunk_size, buffer_size, cache_time);
            
        let inner = StreamManagerInner {
            music_folder: music_folder.to_path_buf(),
            chunk_size,
            buffer_size,
            cache_time,
            current_track_path: None,
            buffer: VecDeque::with_capacity(buffer_size / chunk_size),
            chunk_times: VecDeque::with_capacity(buffer_size / chunk_size),
            active_listeners: 0,
            streaming: false,
            stream_thread: None,
        };
        
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
    
    pub fn start_streaming(&self, track_path: &str) {
        let mut inner = self.inner.lock();
        
        // If already streaming this track, do nothing
        if inner.current_track_path.as_deref() == Some(track_path) && inner.streaming {
            debug!("Already streaming track: {}", track_path);
            return;
        }
        
        info!("Starting to stream track: {}", track_path);
        
        // Clear buffer if switching tracks
        if inner.current_track_path.as_deref() != Some(track_path) {
            inner.buffer.clear();
            inner.chunk_times.clear();
            debug!("Cleared buffer for new track");
        }
        
        inner.current_track_path = Some(track_path.to_string());
        
        // Stop existing stream thread if any
        if inner.streaming && inner.stream_thread.is_some() {
            info!("Stopping existing stream thread");
            inner.streaming = false;
            if let Some(thread) = inner.stream_thread.take() {
                drop(inner); // Release lock before joining
                let _ = thread.join();
                inner = self.inner.lock();
            }
        }
        
        // Start new stream thread
        inner.streaming = true;
        
        let music_folder = inner.music_folder.clone();
        let track_path = track_path.to_string();
        let chunk_size = inner.chunk_size;
        let inner_clone = self.inner.clone();
        
        debug!("Creating new stream thread for track: {}", track_path);
        let thread_handle = thread::spawn(move || {
            Self::buffer_track(inner_clone, &music_folder, &track_path, chunk_size);
        });
        
        inner.stream_thread = Some(thread_handle);
    }
    
    fn buffer_track(
        inner: Arc<Mutex<StreamManagerInner>>, 
        music_folder: &Path, 
        track_path: &str, 
        chunk_size: usize
    ) {
        let file_path = music_folder.join(track_path);
        let start_time = std::time::Instant::now();
        
        println!("Starting to buffer track: {}", file_path.display());
        
        if !file_path.exists() {
            println!("ERROR: File not found: {}", file_path.display());
            let mut inner = inner.lock();
            inner.streaming = false;
            return;
        }
        
        println!("Opening file for streaming: {}", file_path.display());
        let mut file = match File::open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                println!("ERROR: Error opening file {}: {}", file_path.display(), e);
                let mut inner = inner.lock();
                inner.streaming = false;
                return;
            }
        };
        
        // Get file size for logging
        let file_size = match file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        };
        
        println!("Starting to stream file: {}, size: {} bytes", file_path.display(), file_size);
        
        let mut buffer = vec![0; chunk_size];
        let mut total_bytes_read = 0;
        let mut chunks_buffered = 0;
        let mut last_progress_log = std::time::Instant::now();
        
        loop {
            // Check if we should continue streaming
            let should_continue = {
                let inner = inner.lock();
                inner.streaming
            };
            
            if !should_continue {
                println!("Stopping stream thread as requested after {} seconds", start_time.elapsed().as_secs());
                break;
            }
            
            // Log progress every 5 seconds
            if last_progress_log.elapsed().as_secs() >= 5 {
                println!("BUFFER STATUS: Streaming \"{}\" - {} bytes read ({:.2}% of file) over {} seconds", 
                       track_path, 
                       total_bytes_read,
                       if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 },
                       start_time.elapsed().as_secs());
                last_progress_log = std::time::Instant::now();
            }
            
            // Check if buffer needs more data
            let needs_data = {
                let inner = inner.lock();
                inner.buffer.len() < inner.buffer.capacity()
            };
            
            if needs_data {
                match file.read(&mut buffer) {
                    Ok(0) => {
                        // End of file reached
                        let elapsed = start_time.elapsed().as_secs();
                        println!("End of track reached: {} after {} seconds", track_path, elapsed);
                        println!("Total bytes read: {}, chunks buffered: {}, time taken: {}s", 
                               total_bytes_read, chunks_buffered, elapsed);
                        
                        if file_size > 0 {
                            println!("File read complete: {}% of file processed", 
                                   (total_bytes_read as f64 / file_size as f64) * 100.0);
                        }
                        
                        // CRITICAL FIX: Set streaming to false when track is complete
                        let mut inner_lock = inner.lock();
                        inner_lock.streaming = false;
                        println!("Set streaming to false as track has completed after {} seconds", elapsed);
                        break;
                    },
                    Ok(n) => {
                        total_bytes_read += n;
                        chunks_buffered += 1;
                        
                        if chunks_buffered % 100 == 0 {
                            println!("Buffered {} chunks, {} bytes ({:.2}% of file) in {} seconds", 
                                   chunks_buffered, total_bytes_read,
                                   if file_size > 0 { (total_bytes_read as f64 / file_size as f64) * 100.0 } else { 0.0 },
                                   start_time.elapsed().as_secs());
                        }
                        
                        let chunk = buffer[..n].to_vec();
                        let mut inner = inner.lock();
                        inner.buffer.push_back(chunk);
                        inner.chunk_times.push_back(Instant::now());
                    },
                    Err(e) => {
                        println!("ERROR: Error reading file {}: {} after {} seconds", 
                               file_path.display(), e, start_time.elapsed().as_secs());
                        let mut inner = inner.lock();
                        inner.streaming = false;
                        break;
                    }
                }
            } else {
                // Check if oldest chunk needs to be replaced due to cache time
                let should_replace = {
                    let inner = inner.lock();
                    if let Some(oldest_time) = inner.chunk_times.front() {
                        oldest_time.elapsed().as_secs() > inner.cache_time
                    } else {
                        false
                    }
                };
                
                if should_replace {
                    let mut inner = inner.lock();
                    inner.buffer.pop_front();
                    inner.chunk_times.pop_front();
                    println!("Removed oldest chunk from buffer due to cache time, elapsed: {} seconds", 
                           start_time.elapsed().as_secs());
                    continue;
                }
                
                // Buffer is full and not expired, wait a bit
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    pub fn force_stop_streaming(&self) {
        let mut inner = self.inner.lock();
        inner.streaming = false;
        println!("Force stopped streaming by setting streaming flag to false");
    }
    
    pub fn get_stream_generator(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        debug!("Creating stream generator");
        
        // Create stream generator
        StreamGenerator {
            manager: self.clone(),
            position: 0,
        }
    }
    
    pub fn get_active_listeners(&self) -> usize {
        let inner = self.inner.lock();
        inner.active_listeners
    }
    
    pub fn is_streaming(&self) -> bool {
        let inner = self.inner.lock();
        inner.streaming
    }
    
    pub fn increment_listener_count(&self) {
        let mut inner = self.inner.lock();
        inner.active_listeners += 1;
        info!("Listener connected. Active listeners: {}", inner.active_listeners);
    }

    pub fn decrement_listener_count(&self) {
        let mut inner = self.inner.lock();
        if inner.active_listeners > 0 {
            inner.active_listeners -= 1;
        }
        info!("Listener disconnected. Active listeners: {}", inner.active_listeners);
    }
    
    pub fn inner(&self) -> &Self {
        self
    }
    
    pub fn get_current_track_path(&self) -> Option<String> {
        let inner = self.inner.lock();
        inner.current_track_path.clone()
    }
    
    pub fn buffer_status(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        (inner.buffer.len(), inner.buffer.capacity())
    }
}

struct StreamGenerator {
    manager: StreamManager,
    position: usize,
}

impl Drop for StreamGenerator {
    fn drop(&mut self) {
        debug!("StreamGenerator dropped");
    }
}

impl Iterator for StreamGenerator {
    type Item = Vec<u8>;
    
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Get buffer length and check if we should continue
            let (buffer_len, streaming) = {
                let inner = self.manager.inner.lock();
                (inner.buffer.len(), inner.streaming)
            };
            
            if self.position < buffer_len {
                // Get next chunk from buffer
                let chunk = {
                    let inner = self.manager.inner.lock();
                    inner.buffer.get(self.position).cloned()
                };
                
                self.position += 1;
                return chunk;
            } else if !streaming {
                // End of track reached and not streaming anymore
                debug!("End of stream reached at position {}", self.position);
                return None;
            } else {
                // Wait for more data
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}
// src/services/transcoder.rs - Minimal implementation to prevent panics

use std::sync::Arc;
use std::thread;
use std::time::Duration;
use parking_lot::Mutex;
use log::{info, warn, error};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, Ordering};

// Simple structure to hold minimal state
pub struct TranscoderManager {
    pub mp3_buffer: Arc<Mutex<Vec<u8>>>,
    pub opus_buffer: Arc<Mutex<Vec<u8>>>,
    pub opus_broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
    is_transcoding: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    transcoder_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    buffer_size: usize,
    chunk_size: usize,
}

impl TranscoderManager {
    pub fn new(buffer_size: usize, chunk_size: usize) -> Self {
        // Create a broadcast channel for Opus data
        let (tx, _) = broadcast::channel(100);
        
        Self {
            mp3_buffer: Arc::new(Mutex::new(Vec::with_capacity(buffer_size))),
            opus_buffer: Arc::new(Mutex::new(Vec::with_capacity(buffer_size))),
            opus_broadcast_tx: Arc::new(tx),
            is_transcoding: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            transcoder_thread: Arc::new(Mutex::new(None)),
            buffer_size,
            chunk_size,
        }
    }
    
    pub fn get_opus_broadcast_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.opus_broadcast_tx.subscribe()
    }
    
    pub fn is_transcoding(&self) -> bool {
        self.is_transcoding.load(Ordering::SeqCst)
    }
    
    pub fn add_mp3_chunk(&self, chunk: &[u8]) {
        // No actual processing required - just acknowledge the data
        // This prevents memory issues by not using the buffer at all
        info!("Received MP3 chunk of size: {} bytes", chunk.len());
    }
    
    pub fn get_opus_chunks_from_current_position(&self) -> Vec<Vec<u8>> {
        // Just return the sample Opus headers as data
        // This ensures iOS clients get some data
        vec![get_opus_header()]
    }
    
    // Use Arc<Self> to avoid lifetime issues
    pub fn start_transcoding_shared(self: Arc<Self>) {
        // Check if already running
        let thread_guard = self.transcoder_thread.lock();
        if thread_guard.is_some() {
            warn!("Transcoder already running");
            return;
        }
        drop(thread_guard);
        
        info!("Starting transcoding thread");
        self.is_transcoding.store(true, Ordering::SeqCst);
        self.should_stop.store(false, Ordering::SeqCst);
        
        // Clone for the thread
        let this = self.clone();
        
        // Start a very simple thread that just periodically sends dummy Opus data
        let handle = thread::spawn(move || {
            info!("Transcoder thread started - sending dummy Opus data");
            
            // Store dummy Opus header
            {
                let mut buffer = this.opus_buffer.lock();
                buffer.clear();
                let header = get_opus_header();
                buffer.extend_from_slice(&header);
            }
            
            // Send header to all clients
            let _ = this.opus_broadcast_tx.send(get_opus_header());
            
            // Main loop - just periodically send dummy data
            while !this.should_stop.load(Ordering::SeqCst) {
                // Every second, send a dummy packet
                thread::sleep(Duration::from_secs(1));
                
                // Get a dummy Opus packet
                let dummy_packet = get_dummy_opus_packet();
                
                // Broadcast to clients
                let _ = this.opus_broadcast_tx.send(dummy_packet.clone());
                
                // Store in buffer
                {
                    let mut buffer = this.opus_buffer.lock();
                    buffer.extend_from_slice(&dummy_packet);
                    
                    // Cap buffer size
                    if buffer.len() > this.buffer_size && this.buffer_size > 0 {
                        buffer.clear();
                        buffer.extend_from_slice(&get_opus_header());
                    }
                }
            }
            
            info!("Transcoder thread ended");
            this.is_transcoding.store(false, Ordering::SeqCst);
        });
        
        // Store thread handle
        let mut thread_guard = self.transcoder_thread.lock();
        *thread_guard = Some(handle);
    }
    
    // Method for backward compatibility
    pub fn start_transcoding(&mut self) {
        // Create a new Arc for this method
        let arc_self = Arc::new(TranscoderManager {
            mp3_buffer: self.mp3_buffer.clone(),
            opus_buffer: self.opus_buffer.clone(),
            opus_broadcast_tx: self.opus_broadcast_tx.clone(),
            is_transcoding: self.is_transcoding.clone(),
            should_stop: self.should_stop.clone(),
            transcoder_thread: self.transcoder_thread.clone(),
            buffer_size: self.buffer_size,
            chunk_size: self.chunk_size,
        });
        
        arc_self.start_transcoding_shared();
    }
    
    // Send Opus headers
    pub fn send_opus_headers(&self) {
        let header = get_opus_header();
        
        // Store in buffer
        {
            let mut buffer = self.opus_buffer.lock();
            buffer.clear();
            buffer.extend_from_slice(&header);
        }
        
        // Send to clients
        let _ = self.opus_broadcast_tx.send(header);
        info!("Sent Opus headers");
    }
    
    // Stop the transcoder
    pub fn stop_transcoding(&mut self) {
        info!("Stopping transcoder");
        self.should_stop.store(true, Ordering::SeqCst);
        
        // Join thread
        if let Some(handle) = self.transcoder_thread.lock().take() {
            let _ = handle.join();
        }
        
        self.is_transcoding.store(false, Ordering::SeqCst);
    }
    
    // Keep for API compatibility
    pub fn connect_transcoder(&self, _: Arc<Self>) {
        // Just a stub - no functionality needed
    }
}

impl Drop for TranscoderManager {
    fn drop(&mut self) {
        self.stop_transcoding();
    }
}

// Helper function to create a dummy Opus header
fn get_opus_header() -> Vec<u8> {
    // Create a minimal Opus header that iOS can recognize
    let mut header = Vec::with_capacity(19);
    
    // "OpusHead" magic signature
    header.extend_from_slice(b"OpusHead");
    
    // Version byte
    header.push(1);
    
    // Channel count (stereo = 2)
    header.push(2);
    
    // Pre-skip
    header.push(0);
    header.push(0);
    
    // Sample rate (48kHz)
    header.push(0x80);
    header.push(0xBB);
    header.push(0);
    header.push(0);
    
    // Output gain
    header.push(0);
    header.push(0);
    
    // Mapping family
    header.push(0);
    
    header
}

// Helper function to create dummy Opus packet for streaming
fn get_dummy_opus_packet() -> Vec<u8> {
    // Create a minimal Opus packet with valid framing
    let mut packet = Vec::with_capacity(10);
    
    // Opus TOC byte
    packet.push(0xFC);
    
    // Dummy data that will be interpreted as silence
    packet.push(0xFF);
    packet.push(0xFE);
    packet.push(0xFD);
    packet.push(0xFC);
    packet.push(0xFB);
    packet.push(0xFA);
    packet.push(0xF9);
    packet.push(0xF8);
    
    packet
}
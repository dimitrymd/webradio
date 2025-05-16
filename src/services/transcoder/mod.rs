// Complete updated safe implementation for transcoder/mod.rs

use std::io::Cursor;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, warn, error, debug};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, Ordering};

// Use minimp3 instead of Symphonia
use minimp3::{Decoder as MP3Decoder, Frame as MP3Frame, Error as MP3Error};
use ogg::writing::{PacketWriter, PacketWriteEndInfo};
use audiopus::{Application as OpusApplication, SampleRate, Bitrate};
use audiopus::coder::Encoder as OpusEncoder;
use audiopus::Channels;

use crate::config;

// Constants for Opus encoding
const OPUS_SAMPLE_RATE: u32 = 48000;
const OPUS_CHANNELS: u32 = 2;
const OPUS_FRAME_SIZE_MS: u32 = 20; // 20ms frames
const OPUS_BITRATE: i32 = 128000;   // 128 kbps

// Calculate samples per frame
const OPUS_FRAME_SIZE: usize = (OPUS_SAMPLE_RATE * OPUS_FRAME_SIZE_MS / 1000) as usize;

pub struct TranscoderManager {
    pub mp3_buffer: Arc<Mutex<Vec<u8>>>,
    pub opus_buffer: Arc<Mutex<Vec<u8>>>,
    opus_broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
    is_transcoding: Arc<AtomicBool>,
    should_stop: Arc<AtomicBool>,
    transcoder_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    buffer_size: usize,
    chunk_size: usize,
}

impl TranscoderManager {
    pub fn new(buffer_size: usize, chunk_size: usize) -> Self {
        let (opus_broadcast_tx, _) = broadcast::channel(2000);
        
        Self {
            mp3_buffer: Arc::new(Mutex::new(Vec::with_capacity(buffer_size))),
            opus_buffer: Arc::new(Mutex::new(Vec::with_capacity(buffer_size))),
            opus_broadcast_tx: Arc::new(opus_broadcast_tx),
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
        let mut buffer = self.mp3_buffer.lock();
        buffer.extend_from_slice(chunk);
        
        // Cap buffer size
        if buffer.len() > self.buffer_size {
            let excess = buffer.len() - self.buffer_size;
            buffer.drain(0..excess);
        }
    }
    
    pub fn get_opus_chunks_from_current_position(&self) -> Vec<Vec<u8>> {
        let buffer = self.opus_buffer.lock();
        
        // Convert to vector of chunks
        let mut chunks = Vec::new();
        let mut i = 0;
        while i < buffer.len() {
            let end = std::cmp::min(i + self.chunk_size, buffer.len());
            if end > i {
                chunks.push(buffer[i..end].to_vec());
            }
            i = end;
        }
        
        chunks
    }
    
    pub fn start_transcoding_shared(&self) {
        // Check if already running
        {
            let thread_guard = self.transcoder_thread.lock();
            if thread_guard.is_some() {
                warn!("Transcoder thread already running");
                return;
            }
        }
        
        // Clone all the necessary Arc fields for the new thread
        let mp3_buffer = self.mp3_buffer.clone();
        let opus_buffer = self.opus_buffer.clone();
        let opus_broadcast_tx = self.opus_broadcast_tx.clone();
        let is_transcoding = self.is_transcoding.clone();
        let should_stop = self.should_stop.clone();
        let chunk_size = self.chunk_size;
        
        info!("Starting transcoder thread");
        is_transcoding.store(true, Ordering::SeqCst);
        
        // Spawn the thread
        let thread_handle = thread::spawn(move || {
            Self::transcoding_loop(
                mp3_buffer,
                opus_buffer,
                opus_broadcast_tx,
                is_transcoding,
                should_stop,
                chunk_size,
            );
        });
        
        // Update the thread field through the Mutex
        let mut thread_guard = self.transcoder_thread.lock();
        *thread_guard = Some(thread_handle);
    }
    
    // Original method kept for backward compatibility
    pub fn start_transcoding(&mut self) {
        self.start_transcoding_shared();
    }
    
    fn transcoding_loop(
        mp3_buffer: Arc<Mutex<Vec<u8>>>,
        opus_buffer: Arc<Mutex<Vec<u8>>>,
        opus_broadcast_tx: Arc<broadcast::Sender<Vec<u8>>>,
        is_transcoding: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        chunk_size: usize,
    ) {
        info!("Transcoder thread started");
        
        // Create opus encoder
        let mut opus_encoder = match OpusEncoder::new(
            // Convert u32 to SampleRate enum
            match OPUS_SAMPLE_RATE {
                8000 => SampleRate::Hz8000,
                12000 => SampleRate::Hz12000,
                16000 => SampleRate::Hz16000,
                24000 => SampleRate::Hz24000,
                48000 => SampleRate::Hz48000,
                _ => {
                    error!("Unsupported sample rate: {}", OPUS_SAMPLE_RATE);
                    is_transcoding.store(false, Ordering::SeqCst);
                    return;
                }
            },
            Channels::Stereo,
            OpusApplication::Audio,
        ) {
            Ok(mut encoder) => {
                // Set bitrate
                if let Err(e) = encoder.set_bitrate(Bitrate::BitsPerSecond(OPUS_BITRATE)) {
                    error!("Failed to set Opus bitrate: {:?}", e);
                }
                encoder
            },
            Err(e) => {
                error!("Failed to create Opus encoder: {:?}", e);
                is_transcoding.store(false, Ordering::SeqCst);
                return;
            }
        };
        
        // Generate Opus headers
        let opus_id_header = generate_opus_id_header();
        let opus_comment_header = generate_opus_comment_header();
        
        // Set up headers with Ogg container
        let mut header_buffer = Vec::new();
        {
            // Scope for first header writer
            let mut ogg_buffer = Vec::new();
            let mut ogg_writer = PacketWriter::new(&mut ogg_buffer);
            
            // Write ID header
            if let Err(e) = ogg_writer.write_packet(
                &opus_id_header,
                0,
                PacketWriteEndInfo::EndPage,
                0,
            ) {
                error!("Failed to write Opus ID header: {:?}", e);
            }
            
            // Add to header buffer
            header_buffer.extend_from_slice(&ogg_buffer);
        }
        
        {
            // Scope for second header writer
            let mut ogg_buffer = Vec::new();
            let mut ogg_writer = PacketWriter::new(&mut ogg_buffer);
            
            // Write comment header
            if let Err(e) = ogg_writer.write_packet(
                &opus_comment_header,
                0,
                PacketWriteEndInfo::NormalPacket,
                0,
            ) {
                error!("Failed to write Opus comment header: {:?}", e);
            }
            
            // Add to header buffer
            header_buffer.extend_from_slice(&ogg_buffer);
        }
        
        // Add headers to opus buffer and broadcast
        {
            let mut opus_buf = opus_buffer.lock();
            opus_buf.clear();
            opus_buf.extend_from_slice(&header_buffer);
        }
        
        let _ = opus_broadcast_tx.send(header_buffer);
        
        // Main processing loop
        let mut last_process_time = Instant::now();
        
        while !should_stop.load(Ordering::SeqCst) {
            // Get a copy of the current MP3 buffer
            let mp3_data = {
                let buffer = mp3_buffer.lock();
                if buffer.is_empty() {
                    // No data yet, wait a bit
                    drop(buffer);
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                buffer.clone()
            };
            
            // Use minimp3 decoder with safety checks
            debug!("Creating MP3 decoder for buffer of size: {}", mp3_data.len());
            let mut decoder = MP3Decoder::new(Cursor::new(mp3_data));
            
            // Accumulation buffer for Ogg data
            let mut pending_ogg_data = Vec::new();
            
            // Process MP3 frames
            let mut frames_processed = 0;
            loop {
                match decoder.next_frame() {
                    Ok(frame) => {
                        debug!("Decoded MP3 frame: sample rate: {}, channels: {}, samples: {}", 
                              frame.sample_rate, frame.channels, frame.data.len());
                        
                        // Validate the frame before processing
                        if !is_valid_mp3_frame(&frame) {
                            debug!("Skipping invalid frame");
                            continue;
                        }
                        
                        // Process the frame safely
                        process_mp3_frame_safely(&frame, &mut opus_encoder, &mut pending_ogg_data);
                        frames_processed += 1;
                    },
                    Err(MP3Error::Eof) => {
                        // Reached end of buffer, this is normal
                        debug!("Reached end of MP3 buffer after processing {} frames", frames_processed);
                        break;
                    },
                    Err(e) => {
                        // Handle other errors
                        error!("MP3 decoding error: {:?}", e);
                        // If we've processed some frames, we can continue
                        if frames_processed > 0 {
                            debug!("Continuing after error, processed {} frames", frames_processed);
                        } else {
                            // If we haven't processed any frames, pause before retrying
                            thread::sleep(Duration::from_millis(100));
                        }
                        break;
                    }
                }
            }
            
            // Process accumulated Ogg data
            if !pending_ogg_data.is_empty() && 
               (last_process_time.elapsed() > Duration::from_millis(100) || pending_ogg_data.len() > chunk_size * 2) {
                
                // Add to opus buffer
                {
                    let mut opus_buf = opus_buffer.lock();
                    opus_buf.extend_from_slice(&pending_ogg_data);
                    
                    // Cap buffer size
                    if opus_buf.len() > config::BUFFER_SIZE {
                        let excess = opus_buf.len() - config::BUFFER_SIZE;
                        opus_buf.drain(0..excess);
                    }
                }
                
                // Broadcast in chunks
                for chunk in pending_ogg_data.chunks(chunk_size) {
                    let _ = opus_broadcast_tx.send(chunk.to_vec());
                }
                
                debug!("Broadcast {} bytes of Opus data in {} chunks", 
                      pending_ogg_data.len(), 
                      (pending_ogg_data.len() + chunk_size - 1) / chunk_size);
                
                // Reset
                pending_ogg_data.clear();
                last_process_time = Instant::now();
            }
            
            // Short pause to avoid tight loop
            thread::sleep(Duration::from_millis(50));
        }
        
        info!("Transcoder thread exiting");
        is_transcoding.store(false, Ordering::SeqCst);
    }
    
    pub fn stop_transcoding(&mut self) {
        info!("Stopping transcoder");
        
        self.should_stop.store(true, Ordering::SeqCst);
        
        if let Some(thread) = self.transcoder_thread.lock().take() {
            if let Err(e) = thread.join() {
                error!("Error joining transcoder thread: {:?}", e);
            }
        }
        
        self.is_transcoding.store(false, Ordering::SeqCst);
    }
}

impl Drop for TranscoderManager {
    fn drop(&mut self) {
        self.stop_transcoding();
    }
}

// Helper function to validate MP3 frames before processing
fn is_valid_mp3_frame(frame: &MP3Frame) -> bool {
    // Check if the frame data is empty
    if frame.data.is_empty() {
        debug!("Empty frame data");
        return false;
    }
    
    // Check if the frame has the expected number of channels
    // Fixed: Convert frame.channels to usize for comparison
    if frame.channels as usize != OPUS_CHANNELS {
        debug!("Invalid frame channels: got {}, expected {}", 
               frame.channels, OPUS_CHANNELS as i32);
        return false;
    }
    
    // Check if the frame has enough samples
    let min_samples = OPUS_FRAME_SIZE * frame.channels as usize;
    if frame.data.len() < min_samples / 4 {  // Allow at least 1/4 of a frame
        debug!("Frame too small: {} samples, need at least {}", 
               frame.data.len(), min_samples / 4);
        return false;
    }
    
    // Check for anomalies in sample rate
    if frame.sample_rate != OPUS_SAMPLE_RATE as i32 && 
       frame.sample_rate != 44100 && 
       frame.sample_rate != 22050 &&
       frame.sample_rate != 32000 {
        debug!("Unusual sample rate: {}", frame.sample_rate);
        // We'll still process it, just log the warning
    }
    
    true
}

// Helper function to safely process each MP3 frame
fn process_mp3_frame_safely(
    frame: &MP3Frame, 
    opus_encoder: &mut OpusEncoder,
    pending_ogg_data: &mut Vec<u8>
) {
    // The MP3 frame data contains i16 samples
    let i16_samples = &frame.data;
    
    // Calculate the Opus frame size in samples based on channels
    let opus_frame_samples = OPUS_FRAME_SIZE * OPUS_CHANNELS as usize;
    
    // Process in chunks of opus_frame_samples
    for chunk_start in (0..i16_samples.len()).step_by(opus_frame_samples) {
        // Calculate end, ensuring we don't go past the end of the array
        let chunk_end = std::cmp::min(chunk_start + opus_frame_samples, i16_samples.len());
        let chunk_len = chunk_end - chunk_start;
        
        // Skip chunks that are too small
        if chunk_len < opus_frame_samples / 4 {  // At least 1/4 of a full frame
            continue;
        }
        
        // Create a chunk, padding if necessary
        let chunk = if chunk_len < opus_frame_samples {
            // Need to pad
            let mut padded = i16_samples[chunk_start..chunk_end].to_vec();
            padded.resize(opus_frame_samples, 0);
            padded
        } else {
            // Full chunk, no padding needed
            i16_samples[chunk_start..chunk_end].to_vec()
        };
        
        // Process the chunk
        process_single_chunk(&chunk, opus_encoder, pending_ogg_data);
    }
}

// Helper function to process a single chunk of audio samples
fn process_single_chunk(
    chunk: &[i16],
    opus_encoder: &mut OpusEncoder,
    pending_ogg_data: &mut Vec<u8>
) {
    // Safety check
    if chunk.len() != OPUS_FRAME_SIZE * OPUS_CHANNELS as usize {
        error!("Invalid chunk size: got {}, expected {}", 
               chunk.len(), OPUS_FRAME_SIZE * OPUS_CHANNELS as usize);
        return;
    }
    
    // Encode to Opus
    let mut opus_data = vec![0u8; 4000];
    match opus_encoder.encode(chunk, &mut opus_data) {
        Ok(bytes_written) => {
            opus_data.truncate(bytes_written);
            
            // For each chunk, create a separate Ogg writer
            let mut chunk_ogg_buffer = Vec::new();
            let mut chunk_writer = PacketWriter::new(&mut chunk_ogg_buffer);
            
            match chunk_writer.write_packet(
                &opus_data,
                0,
                PacketWriteEndInfo::NormalPacket,
                OPUS_FRAME_SIZE as u64,
            ) {
                Ok(_) => {
                    // Add to pending Ogg data
                    pending_ogg_data.extend_from_slice(&chunk_ogg_buffer);
                },
                Err(e) => {
                    error!("Failed to write Opus packet: {:?}", e);
                }
            }
        },
        Err(e) => {
            error!("Failed to encode Opus frame: {:?}", e);
        }
    }
}

// Helper function to generate Opus ID header
fn generate_opus_id_header() -> Vec<u8> {
    let mut header = Vec::new();
    
    // Magic signature "OpusHead"
    header.extend_from_slice(b"OpusHead");
    
    // Version (1 byte)
    header.push(1);
    
    // Channel count (1 byte)
    header.push(OPUS_CHANNELS as u8);
    
    // Pre-skip (2 bytes, little endian) - typically 3840 at 48kHz
    let pre_skip: u16 = 3840;
    header.push((pre_skip & 0xFF) as u8);
    header.push(((pre_skip >> 8) & 0xFF) as u8);
    
    // Sample rate (4 bytes, little endian)
    header.push((OPUS_SAMPLE_RATE & 0xFF) as u8);
    header.push(((OPUS_SAMPLE_RATE >> 8) & 0xFF) as u8);
    header.push(((OPUS_SAMPLE_RATE >> 16) & 0xFF) as u8);
    header.push(((OPUS_SAMPLE_RATE >> 24) & 0xFF) as u8);
    
    // Output gain (2 bytes, little endian) - 0 for no gain
    header.push(0);
    header.push(0);
    
    // Channel mapping family (1 byte) - 0 for mono/stereo
    header.push(0);
    
    header
}

// Helper function to generate Opus comment header
fn generate_opus_comment_header() -> Vec<u8> {
    let mut header = Vec::new();
    
    // Magic signature "OpusTags"
    header.extend_from_slice(b"OpusTags");
    
    // Vendor string length (4 bytes, little endian)
    let vendor = b"ChillOut Radio Opus Encoder";
    let vendor_len = vendor.len() as u32;
    header.push((vendor_len & 0xFF) as u8);
    header.push(((vendor_len >> 8) & 0xFF) as u8);
    header.push(((vendor_len >> 16) & 0xFF) as u8);
    header.push(((vendor_len >> 24) & 0xFF) as u8);
    
    // Vendor string
    header.extend_from_slice(vendor);
    
    // User comment list length (4 bytes, little endian)
    let comment_count: u32 = 0; // No comments for now
    header.push((comment_count & 0xFF) as u8);
    header.push(((comment_count >> 8) & 0xFF) as u8);
    header.push(((comment_count >> 16) & 0xFF) as u8);
    header.push(((comment_count >> 24) & 0xFF) as u8);
    
    // No comment entries
    
    header
}
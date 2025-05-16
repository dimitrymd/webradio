// src/transcoder/mod.rs

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use log::{info, warn, error, debug};
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use symphonia::core::audio::{SampleBuffer, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use ogg::writing::PacketWriter;
use audiopus::{Encoder as OpusEncoder, Application as OpusApplication, Channels, Error as OpusError};

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
    transcoder_thread: Option<thread::JoinHandle<()>>,
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
            transcoder_thread: None,
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
    
    pub fn start_transcoding(&mut self) {
        if self.transcoder_thread.is_some() {
            warn!("Transcoder thread already running");
            return;
        }
        
        let mp3_buffer = self.mp3_buffer.clone();
        let opus_buffer = self.opus_buffer.clone();
        let opus_broadcast_tx = self.opus_broadcast_tx.clone();
        let is_transcoding = self.is_transcoding.clone();
        let should_stop = self.should_stop.clone();
        let chunk_size = self.chunk_size;
        
        info!("Starting transcoder thread");
        is_transcoding.store(true, Ordering::SeqCst);
        
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
        
        self.transcoder_thread = Some(thread_handle);
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
            OPUS_SAMPLE_RATE,
            Channels::Stereo,
            OpusApplication::Audio,
        ) {
            Ok(encoder) => {
                // Set bitrate
                if let Err(e) = encoder.set_bitrate(audiopus::Bitrate::Bits(OPUS_BITRATE)) {
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
        
        // Create Ogg packet writer for headers
        let mut ogg_packet_writer = match PacketWriter::new() {
            Ok(writer) => writer,
            Err(e) => {
                error!("Failed to create Ogg packet writer: {:?}", e);
                is_transcoding.store(false, Ordering::SeqCst);
                return;
            }
        };
        
        // Generate and broadcast Opus/Ogg headers
        let opus_id_header = generate_opus_id_header();
        let opus_comment_header = generate_opus_comment_header();
        
        // Write header packets
        let mut header_buffer = Vec::new();
        if let Err(e) = ogg_packet_writer.write_packet(
            opus_id_header.as_slice(), 
            true, // bos (beginning of stream)
            false // eos (end of stream)
        ) {
            error!("Failed to write Opus ID header: {:?}", e);
        }
        
        if let Err(e) = ogg_packet_writer.write_packet(
            opus_comment_header.as_slice(),
            false, // bos
            false  // eos
        ) {
            error!("Failed to write Opus comment header: {:?}", e);
        }
        
        // Get header data
        header_buffer.extend_from_slice(&ogg_packet_writer.inner_mut());
        
        // Add headers to opus buffer
        {
            let mut opus_buf = opus_buffer.lock();
            opus_buf.clear();
            opus_buf.extend_from_slice(&header_buffer);
        }
        
        // Broadcast headers
        let _ = opus_broadcast_tx.send(header_buffer);
        
        // Create a new packet writer for audio data
        let mut ogg_packet_writer = match PacketWriter::new() {
            Ok(writer) => writer,
            Err(e) => {
                error!("Failed to create Ogg packet writer for audio: {:?}", e);
                is_transcoding.store(false, Ordering::SeqCst);
                return;
            }
        };
        
        // Main transcoding loop
        let mut last_process_time = Instant::now();
        let mut decoder_initialized = false;
        let mut symphonia_decoder = None;
        let mut symphonia_format = None;
        
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
            
            // Initialize decoder if needed
            if !decoder_initialized {
                // Create media source
                let source = Cursor::new(mp3_data.clone());
                let mss = MediaSourceStream::new(Box::new(source), Default::default());
                
                // Create a hint to help the format registry guess the format
                let mut hint = Hint::new();
                hint.with_extension("mp3");
                
                // Use the default options for metadata and format readers
                let format_opts = FormatOptions::default();
                let metadata_opts = MetadataOptions::default();
                let decoder_opts = DecoderOptions::default();
                
                // Probe the media source
                match symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts) {
                    Ok(probed) => {
                        // Get the format reader
                        let format = probed.format;
                        
                        // Get the default track
                        let track = match format.default_track() {
                            Some(track) => track,
                            None => {
                                error!("No default track found in MP3 data");
                                thread::sleep(Duration::from_millis(500));
                                continue;
                            }
                        };
                        
                        // Create a decoder for the track
                        match symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts) {
                            Ok(dec) => {
                                symphonia_decoder = Some(dec);
                                symphonia_format = Some(format);
                                decoder_initialized = true;
                                debug!("Decoder initialized successfully");
                            },
                            Err(e) => {
                                error!("Failed to create decoder: {:?}", e);
                                thread::sleep(Duration::from_millis(500));
                                continue;
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to probe MP3 data: {:?}", e);
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                }
            }
            
            // Process audio with the initialized decoder
            if decoder_initialized && symphonia_decoder.is_some() && symphonia_format.is_some() {
                let decoder = symphonia_decoder.as_mut().unwrap();
                let format = symphonia_format.as_mut().unwrap();
                
                // Read and decode packets
                match format.next_packet() {
                    Ok(packet) => {
                        // Decode the packet
                        match decoder.decode(&packet) {
                            Ok(decoded) => {
                                // Get the decoded audio buffer
                                let spec = *decoded.spec();
                                let duration = decoded.capacity() as u64;
                                
                                // Create a sample buffer for the decoded audio
                                let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
                                
                                // Copy the decoded audio to the sample buffer
                                sample_buf.copy_interleaved_ref(decoded);
                                let samples = sample_buf.samples();
                                
                                // Convert to i16 samples for Opus
                                let mut i16_samples = Vec::with_capacity(samples.len());
                                for &sample in samples {
                                    let i16_sample = (sample * 32767.0).round() as i16;
                                    i16_samples.push(i16_sample);
                                }
                                
                                // Encode to Opus
                                for chunk in i16_samples.chunks(OPUS_FRAME_SIZE * OPUS_CHANNELS as usize) {
                                    if chunk.len() < OPUS_FRAME_SIZE * OPUS_CHANNELS as usize {
                                        // Pad with zeros if needed
                                        let mut padded_chunk = Vec::from(chunk);
                                        padded_chunk.resize(OPUS_FRAME_SIZE * OPUS_CHANNELS as usize, 0);
                                        
                                        // Encode
                                        let mut opus_data = vec![0u8; 4000]; // Buffer for opus data
                                        match opus_encoder.encode(&padded_chunk, &mut opus_data) {
                                            Ok(bytes_written) => {
                                                opus_data.truncate(bytes_written);
                                                
                                                // Write to Ogg container
                                                if let Err(e) = ogg_packet_writer.write_packet(
                                                    &opus_data, 
                                                    false, // bos
                                                    false  // eos
                                                ) {
                                                    error!("Failed to write Opus packet: {:?}", e);
                                                }
                                            },
                                            Err(e) => {
                                                error!("Failed to encode Opus frame: {:?}", e);
                                            }
                                        }
                                    } else {
                                        // Encode full-sized chunk
                                        let mut opus_data = vec![0u8; 4000]; // Buffer for opus data
                                        match opus_encoder.encode(chunk, &mut opus_data) {
                                            Ok(bytes_written) => {
                                                opus_data.truncate(bytes_written);
                                                
                                                // Write to Ogg container
                                                if let Err(e) = ogg_packet_writer.write_packet(
                                                    &opus_data, 
                                                    false, // bos
                                                    false  // eos
                                                ) {
                                                    error!("Failed to write Opus packet: {:?}", e);
                                                }
                                            },
                                            Err(e) => {
                                                error!("Failed to encode Opus frame: {:?}", e);
                                            }
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Failed to decode packet: {:?}", e);
                            }
                        }
                    },
                    Err(symphonia::core::errors::Error::IoError(_)) |
                    Err(symphonia::core::errors::Error::ResetRequired) => {
                        // Need to reset the decoder - recreate next iteration
                        decoder_initialized = false;
                        symphonia_decoder = None;
                        symphonia_format = None;
                        debug!("Decoder reset required");
                        continue;
                    },
                    Err(e) => {
                        error!("Failed to get next packet: {:?}", e);
                        // Brief pause to avoid tight loop on repeated errors
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                
                // Get the Ogg data and broadcast in chunks
                if last_process_time.elapsed() > Duration::from_millis(100) {
                    let ogg_data = ogg_packet_writer.inner().to_vec();
                    
                    if !ogg_data.is_empty() {
                        // Add to opus buffer
                        {
                            let mut opus_buf = opus_buffer.lock();
                            opus_buf.extend_from_slice(&ogg_data);
                            
                            // Cap buffer size
                            if opus_buf.len() > config::BUFFER_SIZE {
                                let excess = opus_buf.len() - config::BUFFER_SIZE;
                                opus_buf.drain(0..excess);
                            }
                        }
                        
                        // Broadcast in chunks
                        for chunk in ogg_data.chunks(chunk_size) {
                            let _ = opus_broadcast_tx.send(chunk.to_vec());
                        }
                        
                        // Reset the packet writer to avoid excessive memory usage
                        ogg_packet_writer = match PacketWriter::new() {
                            Ok(writer) => writer,
                            Err(e) => {
                                error!("Failed to create new Ogg packet writer: {:?}", e);
                                break;
                            }
                        };
                    }
                    
                    last_process_time = Instant::now();
                }
            }
        }
        
        info!("Transcoder thread exiting");
        is_transcoding.store(false, Ordering::SeqCst);
    }
    
    pub fn stop_transcoding(&mut self) {
        info!("Stopping transcoder");
        
        self.should_stop.store(true, Ordering::SeqCst);
        
        if let Some(thread) = self.transcoder_thread.take() {
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
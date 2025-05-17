// src/services/websocket_bus.rs - Complete updated file with buffering improvements

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::Mutex;
use rocket_ws as ws;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use log::{info, error, debug, warn};

use crate::services::streamer::StreamManager;

// Improved constants for better client experience
const IMPROVED_INITIAL_CHUNKS_TO_SEND: usize = 150;  // Increased for better initial buffering
const CHUNK_SEND_DELAY_MS: u64 = 0;                 // Removed delay between chunks for faster transmission
const CLIENT_PING_INTERVAL_MS: u64 = 2000;          // More frequent pings for better connection monitoring
const CLIENT_HEALTH_CHECK_INTERVAL_SECS: u64 = 5;   // More frequent health checks

// A client connection with metadata
struct ClientConnection {
    tx: mpsc::UnboundedSender<ws::Message>,
    last_activity: std::time::Instant,
    chunks_sent: usize,
    id3_header_sent: bool,
    initial_chunks_sent: bool,
    buffer_level: usize,       // Track client buffer level
    connection_quality: u8,    // 0-100 quality indicator
}

// WebSocketBus manages all client connections
pub struct WebSocketBus {
    // Map of client ID to client connection
    clients: Arc<Mutex<HashMap<usize, ClientConnection>>>,
    client_counter: AtomicUsize,
    active_listeners: Arc<AtomicUsize>,
    stream_manager: Arc<StreamManager>,
}

impl WebSocketBus {
    pub fn new(stream_manager: Arc<StreamManager>) -> Self {
        WebSocketBus {
            clients: Arc::new(Mutex::new(HashMap::new())),
            client_counter: AtomicUsize::new(0),
            active_listeners: Arc::new(AtomicUsize::new(0)),
            stream_manager,
        }
    }

    pub fn get_stream_manager(&self) -> Arc<StreamManager> {
        self.stream_manager.clone()
    }

    // Add a new client connection
    pub fn add_client(&self) -> (usize, mpsc::UnboundedReceiver<ws::Message>) {
        let client_id = self.client_counter.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        
        // Create client record
        let client = ClientConnection {
            tx,
            last_activity: std::time::Instant::now(),
            chunks_sent: 0,
            id3_header_sent: false,
            initial_chunks_sent: false,
            buffer_level: 0,
            connection_quality: 100, // Assume good quality initially
        };
        
        // Add to clients map
        self.clients.lock().insert(client_id, client);
        
        // Update listener count
        self.increment_listener_count();
        
        info!("Client {} added. Total clients: {}", client_id, self.get_client_count());
        (client_id, rx)
    }

    // Remove a client connection
    pub fn remove_client(&self, client_id: usize) {
        let removed = {
            let mut clients = self.clients.lock();
            clients.remove(&client_id).is_some()
        };
        
        if removed {
            // Update listener count
            self.decrement_listener_count();
            info!("Client {} removed. Total clients: {}", client_id, self.get_client_count());
        }
    }

    // Send a message to a specific client with improved error handling
    pub fn send_to_client(&self, client_id: usize, message: ws::Message) -> bool {
        let mut clients = self.clients.lock();
        if let Some(client) = clients.get_mut(&client_id) {
            client.last_activity = std::time::Instant::now();
            
            // Track message type for buffer management
            if let ws::Message::Binary(_) = &message {
                client.chunks_sent += 1;
                client.buffer_level += 1;
            }
            
            // Try sending with better error handling
            match client.tx.send(message) {
                Ok(_) => {
                    // Successfully sent
                    true
                },
                Err(e) => {
                    error!("Failed to send to client {}: {}", client_id, e);
                    // Mark connection quality as poor
                    client.connection_quality = 0;
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn broadcast_text(&self, text: &str) {
        let message = ws::Message::Text(text.to_string());
        let clients = self.clients.lock();
        
        // Send the message to each client with error tracking
        let mut failed_clients = Vec::new();
        
        for (client_id, client) in clients.iter() {
            if let Err(e) = client.tx.send(message.clone()) {
                error!("Error broadcasting text to client {}: {}", client_id, e);
                failed_clients.push(*client_id);
            }
        }
        
        // Don't remove clients here to avoid deadlock
        // They'll be removed in the health check
        if !failed_clients.is_empty() {
            warn!("Failed to send text message to {} clients", failed_clients.len());
        }
    }

    pub fn broadcast_now_playing(&self) {
        // Get track info from stream manager
        let track_info = self.stream_manager.get_track_info();
        let playback_position = self.stream_manager.get_playback_position();
        let active_listeners = self.get_active_listeners();
        let current_bitrate = self.stream_manager.get_current_bitrate();
        let playback_percentage = self.stream_manager.get_playback_percentage();
        
        if let Some(track_json) = track_info {
            if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(&track_json) {
                if let serde_json::Value::Object(ref mut map) = track_value {
                    map.insert(
                        "active_listeners".to_string(), 
                        serde_json::Value::Number(serde_json::Number::from(active_listeners))
                    );
                    map.insert(
                        "playback_position".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(playback_position))
                    );
                    map.insert(
                        "bitrate".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(current_bitrate / 1000))
                    );
                    map.insert(
                        "percentage".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(playback_percentage))
                    );
                }
                
                // Create the now playing message
                let now_playing_message = serde_json::json!({
                    "type": "now_playing",
                    "track": track_value
                });
                
                // Broadcast to all clients
                if let Ok(message_text) = serde_json::to_string(&now_playing_message) {
                    self.broadcast_text(&message_text);
                }
            }
        }
    }

    // Improved initial data sending with better buffering
    pub async fn send_initial_data(&self, client_id: usize) -> bool {
        let stream_manager = &self.stream_manager;
        let track_info = stream_manager.get_track_info();
        
        // Get current track info and playback position
        let current_position = stream_manager.get_playback_position();
        let playback_percentage = stream_manager.get_playback_percentage();
        
        // Get the ID3 header and the most recent chunks
        let (id3_header, saved_chunks) = stream_manager.get_chunks_from_current_position();
        
        // Mark client as having received ID3 header
        {
            let mut clients = self.clients.lock();
            if let Some(client) = clients.get_mut(&client_id) {
                client.id3_header_sent = true;
            } else {
                return false;
            }
        }
        
        // Send track info with current position
        if let Some(info) = track_info {
            if let Ok(mut track_value) = serde_json::from_str::<serde_json::Value>(&info) {
                // Add current position info to the track data
                if let serde_json::Value::Object(ref mut map) = track_value {
                    map.insert(
                        "playback_position".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(current_position))
                    );
                    map.insert(
                        "percentage".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(playback_percentage))
                    );
                    // Add flag to indicate this is a mid-stream join
                    map.insert(
                        "mid_stream_join".to_string(),
                        serde_json::Value::Bool(current_position > 0)
                    );
                }
                
                // Convert to string and send
                if let Ok(modified_info) = serde_json::to_string(&track_value) {
                    if !self.send_to_client(client_id, ws::Message::Text(modified_info)) {
                        return false;
                    }
                } else {
                    // Fall back to original info if serialization fails
                    if !self.send_to_client(client_id, ws::Message::Text(info)) {
                        return false;
                    }
                }
            } else {
                // If parsing failed, send original info
                if !self.send_to_client(client_id, ws::Message::Text(info)) {
                    return false;
                }
            }
        }
        
        // Send ID3 header
        if let Some(id3) = id3_header {
            if !self.send_to_client(client_id, ws::Message::Binary(id3)) {
                return false;
            }
            
            // No pause - we want to send data as quickly as possible
        }
        
        // Calculate which chunks to send based on current position
        // We need to find chunks that are close to the current playback position
        let mut chunks_to_send = Vec::new();
        
        if playback_percentage > 0 && !saved_chunks.is_empty() {
            // Estimate which chunk corresponds to current position
            // We want to start a bit before current position to avoid choppy start
            let total_chunks = saved_chunks.len();
            let target_chunk_index = (total_chunks as f32 * (playback_percentage as f32 / 100.0)) as usize;
            
            // Start more chunks before current position (increased buffer)
            let buffer_chunks = 20; // Increased from 10
            let start_index = if target_chunk_index > buffer_chunks {
                target_chunk_index - buffer_chunks
            } else {
                0
            };
            
            // Select the chunks to send
            let max_initial_chunks = IMPROVED_INITIAL_CHUNKS_TO_SEND;
            let end_index = std::cmp::min(start_index + max_initial_chunks, total_chunks);
            
            // Take chunks from calculated position, not from the beginning
            chunks_to_send = saved_chunks[start_index..end_index].to_vec();
            
            debug!("Sending chunks from position {}% (index {}-{} of {})", 
                  playback_percentage, start_index, end_index, total_chunks);
        } else {
            // If we can't determine position, send initial chunks as before
            let initial_chunks = std::cmp::min(saved_chunks.len(), IMPROVED_INITIAL_CHUNKS_TO_SEND);
            chunks_to_send = saved_chunks.iter().take(initial_chunks).cloned().collect();
            debug!("Sending {} initial chunks to client {}", chunks_to_send.len(), client_id);
        }
        
        // Send the selected chunks in one burst without delays
        for chunk in &chunks_to_send {
            if !chunk.is_empty() {
                if !self.send_to_client(client_id, ws::Message::Binary(chunk.clone())) {
                    return false;
                }
                // No delay - send as fast as possible
            }
        }
        
        // Mark client as having received initial chunks
        {
            let mut clients = self.clients.lock();
            if let Some(client) = clients.get_mut(&client_id) {
                client.initial_chunks_sent = true;
                // Set initial buffer level
                client.buffer_level = chunks_to_send.len();
            }
        }
        
        true
    }

    // Broadcast a message to all clients with improved flow control
    pub fn broadcast(&self, message: ws::Message) {
        let clients = self.clients.lock();
        
        // Calculate message size for buffer tracking
        let msg_size = match &message {
            ws::Message::Binary(data) => data.len(),
            _ => 0,
        };
        
        let is_binary = matches!(message, ws::Message::Binary(_));
        
        // Send the message to each client with better flow control
        for (client_id, client) in clients.iter() {
            // Update buffer level if it's audio data
            if is_binary && msg_size > 0 {
                // Adjust sending based on estimated client buffer
                let buffer_level = client.buffer_level;
                
                // Throttle sending for clients with very high buffer - less aggressive throttling
                if buffer_level > IMPROVED_INITIAL_CHUNKS_TO_SEND * 3 {
                    // Skip every third chunk for clients with very high buffer
                    if client.chunks_sent % 3 != 0 {
                        continue;
                    }
                }
            }
            
            if let Err(e) = client.tx.send(message.clone()) {
                error!("Error broadcasting to client {}: {}", client_id, e);
                // Note: We don't remove clients here to avoid deadlock
                // They'll be removed when their receiver task dies
            }
        }
    }

    // Get the count of connected clients
    pub fn get_client_count(&self) -> usize {
        self.clients.lock().len()
    }

    // Improved health check with buffer management
    pub fn perform_health_check(&self) {
        let now = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(crate::config::WS_TIMEOUT_SECS);
        
        // Identify clients to remove (to avoid deadlock)
        let clients_to_remove: Vec<usize> = {
            let clients = self.clients.lock();
            clients.iter()
                .filter(|(_, client)| now.duration_since(client.last_activity) > timeout)
                .map(|(id, _)| *id)
                .collect()
        };
        
        // Update buffer levels for all clients
        {
            let mut clients = self.clients.lock();
            for (_, client) in clients.iter_mut() {
                // Simulate buffer consumption - client has played some audio
                if client.buffer_level > 0 {
                    // Reduce buffer by a reasonable amount based on time passed
                    let buffer_reduction = client.buffer_level / 10 + 1;
                    client.buffer_level = client.buffer_level.saturating_sub(buffer_reduction);
                }
            }
        }
        
        // Remove timed-out clients
        for client_id in clients_to_remove {
            warn!("Client {} timed out after {}s", client_id, timeout.as_secs());
            self.remove_client(client_id);
        }
    }

    // Send ping messages to keep connections alive
    pub fn ping_clients(&self) {
        let ping_message = ws::Message::Ping(vec![]);
        
        let clients = self.clients.lock();
        for (client_id, client) in clients.iter() {
            if let Err(e) = client.tx.send(ping_message.clone()) {
                error!("Error sending ping to client {}: {}", client_id, e);
            }
        }
    }

    // Update client activity timestamp
    pub fn update_client_activity(&self, client_id: usize) {
        let mut clients = self.clients.lock();
        if let Some(client) = clients.get_mut(&client_id) {
            client.last_activity = std::time::Instant::now();
        }
    }

    // For active listener count tracking
    fn increment_listener_count(&self) {
        let new_count = self.active_listeners.fetch_add(1, Ordering::SeqCst) + 1;
        info!("Listener connected. Active: {}", new_count);
        
        // Also update the StreamManager's count
        self.stream_manager.increment_listener_count();
    }

    fn decrement_listener_count(&self) {
        let prev = self.active_listeners.load(Ordering::SeqCst);
        if prev > 0 {
            let new_count = self.active_listeners.fetch_sub(1, Ordering::SeqCst) - 1;
            info!("Listener disconnected. Active: {}", new_count);
            
            // Also update the StreamManager's count
            self.stream_manager.decrement_listener_count();
        }
    }

    // Get the active listener count
    pub fn get_active_listeners(&self) -> usize {
        self.active_listeners.load(Ordering::SeqCst)
    }

    // Start the broadcast loop in its own task
    pub fn start_broadcast_loop(self: Arc<Self>) {
        // This method is now just a placeholder that does nothing
        // We'll use start_broadcast_loop_impl directly from the Rocket fairing
        info!("WebSocket broadcast loop will be started by Rocket runtime");
    }

    // The actual implementation with improved client handling
    pub async fn start_broadcast_loop_impl(self: Arc<Self>) {
        tokio::spawn(async move {
            info!("Starting WebSocket broadcast loop");
            
            // Get broadcast receiver from StreamManager
            let stream_manager = self.stream_manager.clone();
            let mut broadcast_rx = stream_manager.get_broadcast_receiver();
            
            // Set up ping timer with more frequent pings
            let ping_interval = tokio::time::Duration::from_millis(CLIENT_PING_INTERVAL_MS);
            let mut ping_timer = tokio::time::interval(ping_interval);
            
            // Set up health check timer with more frequent checks
            let health_check_interval = tokio::time::Duration::from_secs(CLIENT_HEALTH_CHECK_INTERVAL_SECS);
            let mut health_check_timer = tokio::time::interval(health_check_interval);
            
            // Set up now playing update timer
            let now_playing_interval = tokio::time::Duration::from_secs(10);
            let mut now_playing_timer = tokio::time::interval(now_playing_interval);
            
            loop {
                tokio::select! {
                    // Handle ping timer
                    _ = ping_timer.tick() => {
                        self.ping_clients();
                    }
                    
                    // Handle health check timer
                    _ = health_check_timer.tick() => {
                        self.perform_health_check();
                    }
                    
                    // Handle now playing update timer
                    _ = now_playing_timer.tick() => {
                        self.broadcast_now_playing();
                    }
                    
                    // Receive chunks from broadcast with better error handling
                    chunk_result = broadcast_rx.recv() => {
                        match chunk_result {
                            Ok(chunk) => {
                                // Broadcast the chunk to all clients
                                let client_count = self.get_client_count();
                                if client_count > 0 {
                                    let message = ws::Message::Binary(chunk);
                                    self.broadcast(message);
                                }
                            },
                            Err(e) => {
                                // Handle broadcast errors more robustly
                                if e.to_string().contains("lagged") {
                                    warn!("Broadcast receiver lagged, resubscribing");
                                    // Get a fresh receiver and continue
                                    broadcast_rx = stream_manager.get_broadcast_receiver();
                                } else {
                                    error!("Broadcast error: {}", e);
                                    // Brief pause before retrying
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
// New file: websocket_bus.rs
// Provides a centralized message bus for WebSocket connections

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::Mutex;
use rocket_ws as ws;
use tokio::sync::mpsc;
use log::{info, error, debug, warn};

use crate::services::streamer::StreamManager;

// A client connection with metadata
struct ClientConnection {
    tx: mpsc::UnboundedSender<ws::Message>,
    last_activity: std::time::Instant,
    chunks_sent: usize,
    id3_header_sent: bool,
    initial_chunks_sent: bool,
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

    // Send a message to a specific client
    pub fn send_to_client(&self, client_id: usize, message: ws::Message) -> bool {
        let mut clients = self.clients.lock();
        if let Some(client) = clients.get_mut(&client_id) {
            client.last_activity = std::time::Instant::now();
            
            // Track message type
            if let ws::Message::Binary(_) = &message {
                client.chunks_sent += 1;
            }
            
            // Send the message
            match client.tx.send(message) {
                Ok(_) => true,
                Err(e) => {
                    error!("Failed to send to client {}: {}", client_id, e);
                    false
                }
            }
        } else {
            false
        }
    }

    // Send initial setup data to a new client
    pub fn send_initial_data(&self, client_id: usize) -> bool {
        let stream_manager = &self.stream_manager;
        let track_info = stream_manager.get_track_info();
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
        
        // Send track info
        if let Some(info) = track_info {
            if !self.send_to_client(client_id, ws::Message::Text(info)) {
                return false;
            }
        }
        
        // Send ID3 header
        if let Some(id3) = id3_header {
            if !self.send_to_client(client_id, ws::Message::Binary(id3)) {
                return false;
            }
        }
        
        // Send initial chunks
        let initial_chunks = std::cmp::min(saved_chunks.len(), crate::config::INITIAL_CHUNKS_TO_SEND);
        debug!("Sending {} initial chunks to client {}", initial_chunks, client_id);
        
        for chunk in saved_chunks.iter().take(initial_chunks) {
            if !chunk.is_empty() {
                if !self.send_to_client(client_id, ws::Message::Binary(chunk.clone())) {
                    return false;
                }
            }
        }
        
        // Mark client as having received initial chunks
        {
            let mut clients = self.clients.lock();
            if let Some(client) = clients.get_mut(&client_id) {
                client.initial_chunks_sent = true;
            }
        }
        
        true
    }

    // Broadcast a message to all clients
    pub fn broadcast(&self, message: ws::Message) {
        let clients = self.clients.lock();
        
        // Send the message to each client
        for (client_id, client) in clients.iter() {
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

    // Periodically check client health
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
    // This method is used externally but doesn't spawn a task directly
    pub fn start_broadcast_loop(self: Arc<Self>) {
        // This method is now just a placeholder that does nothing
        // We'll use start_broadcast_loop_impl directly from the Rocket fairing
        info!("WebSocket broadcast loop will be started by Rocket runtime");
    }

    // The actual implementation that will be called from within Rocket's context
    pub async fn start_broadcast_loop_impl(self: Arc<Self>) {
        info!("Starting WebSocket broadcast loop in Rocket runtime context");
        
        // Get broadcast receiver from StreamManager
        let stream_manager = self.stream_manager.clone();
        let mut broadcast_rx = stream_manager.get_broadcast_receiver();
        
        // Set up ping timer
        let ping_interval = tokio::time::Duration::from_millis(crate::config::WS_PING_INTERVAL_MS);
        let mut ping_timer = tokio::time::interval(ping_interval);
        
        // Set up health check timer
        let health_check_interval = tokio::time::Duration::from_secs(10); // Check every 10 seconds
        let mut health_check_timer = tokio::time::interval(health_check_interval);
        
        loop {
            tokio::select! {
                // Handle ping timer
                _ = ping_timer.tick() => {
                    // Send ping to all clients
                    self.ping_clients();
                }
                
                // Handle health check timer
                _ = health_check_timer.tick() => {
                    // Perform health check
                    self.perform_health_check();
                }
                
                // Receive chunks from broadcast
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
                            // Handle broadcast errors
                            if e.to_string().contains("lagged") {
                                warn!("Broadcast receiver lagged, resubscribing");
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
    }
}
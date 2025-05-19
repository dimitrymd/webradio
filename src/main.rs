// main.rs - Completely rewritten to fix the duplicate module issues

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers};
use std::thread;
use std::sync::Arc;

mod config;
mod handlers;
mod models;
mod services;
mod utils;
mod direct_stream; // Import the direct stream module

use crate::services::streamer::StreamManager;
use crate::services::websocket_bus::WebSocketBus;
use crate::services::playlist;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    println!("============================================================");
    println!("Starting Rust MP3 Web Radio (Direct Streaming Architecture)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("Chunk size: {}, Buffer size: {}", config::CHUNK_SIZE, config::BUFFER_SIZE);
    println!("============================================================");

    // Initialize the stream manager with the configuration values
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // We still need WebSocketBus for StreamManager integration, 
    // but clients won't connect to it
    let websocket_bus = Arc::new(WebSocketBus::new(stream_manager.clone()));
    
    // Rescan and update durations before starting
    println!("Checking and updating track durations...");
    playlist::rescan_and_update_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Ensure we have tracks
    let has_tracks = if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
        println!("Initial track: {} (duration: {}s)", track.title, track.duration);
        true
    } else {
        println!("WARNING: No tracks available for initial playback");
        
        // Try to scan music folder for new tracks
        println!("Scanning music folder for new tracks...");
        playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
        
        // Check again
        if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            println!("Found track after scanning: {} (duration: {}s)", track.title, track.duration);
            true
        } else {
            println!("ERROR: No tracks available after scanning");
            false
        }
    };
    
    // Start the broadcast thread only if we have tracks
    if has_tracks {
        println!("Starting broadcast thread...");
        stream_manager.start_broadcast_thread();
    } else {
        println!("Not starting broadcast thread - no tracks available");
    }

    // Start monitoring thread to handle track switching
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        println!("Starting track monitoring thread...");
        crate::services::playlist::track_switcher(stream_manager_for_monitor);
    });
    
    println!("Server initialization complete, starting web server...");
    
    // Build and launch the Rocket instance
    rocket::build()
        .manage(stream_manager.clone())
        .manage(websocket_bus.clone())
        .attach(WebSocketFairing)  // Keep this for WebSocketBus initialization
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_stats,
            // Commenting out the WebSocket endpoint since we're using direct streaming only
            // handlers::stream_ws,  
            direct_stream::direct_stream,
            direct_stream::direct_stream_head,
            direct_stream::stream_status,
            handlers::static_files,
            handlers::diagnostic_page,
        ])
        .register("/", catchers![
            handlers::not_found,
            handlers::server_error,
            handlers::service_unavailable,
        ])
        .attach(Template::fairing())
}

// Keeping the WebSocketFairing for compatibility with existing code
// We can remove this later when fully migrating away from WebSockets
struct WebSocketFairing;

#[rocket::async_trait]
impl rocket::fairing::Fairing for WebSocketFairing {
    fn info(&self) -> rocket::fairing::Info {
        rocket::fairing::Info {
            name: "WebSocket Broadcast Loop",
            kind: rocket::fairing::Kind::Ignite
        }
    }

    async fn on_ignite(&self, rocket: rocket::Rocket<rocket::Build>) -> rocket::fairing::Result {
        // Get the WebSocketBus from managed state
        if let Some(websocket_bus) = rocket.state::<Arc<WebSocketBus>>() {
            println!("Starting WebSocket broadcast loop from Rocket runtime...");
            
            // Clone the bus and start the broadcast loop
            let bus = websocket_bus.clone();
            tokio::spawn(async move {
                // Start the WebSocket broadcasting
                bus.start_broadcast_loop_impl().await;
            });
        } else {
            println!("WARNING: Could not find WebSocketBus in Rocket state");
        }
        
        Ok(rocket)
    }
}
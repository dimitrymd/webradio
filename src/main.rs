// Updated main.rs with safer transcoder implementation
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

use crate::services::streamer::StreamManager;
use crate::services::websocket_bus::WebSocketBus;
use crate::services::playlist;
use crate::services::transcoder::TranscoderManager;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    println!("============================================================");
    println!("Starting Rust MP3 Web Radio (iOS Compatible with Opus Transcoding)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("Chunk size: {}, Buffer size: {}", config::CHUNK_SIZE, config::BUFFER_SIZE);
    println!("Transcoding enabled: {}", config::ENABLE_TRANSCODING);
    println!("============================================================");

    // Initialize the stream manager with the configuration values
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Initialize the transcoder for iOS with safer implementation
    let transcoder = Arc::new(TranscoderManager::new(
        config::OPUS_BUFFER_SIZE,
        config::OPUS_CHUNK_SIZE,
    ));
    
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
    
    // Create WebSocket bus for optimized handling
    let websocket_bus = Arc::new(WebSocketBus::new(stream_manager.clone()));
    
    // Start WebSocket broadcast loop in a separate task
    println!("Starting WebSocket broadcast loop...");
    websocket_bus.clone().start_broadcast_loop();
    
    // Start the broadcast thread only if we have tracks
    if has_tracks {
        println!("Starting broadcast thread...");
        stream_manager.start_broadcast_thread();
        
        // Also start transcoder if enabled
        if config::ENABLE_TRANSCODING {
            println!("Starting MP3 to Opus transcoder (minimal implementation)...");
            
            // First send headers
            transcoder.send_opus_headers();
            
            // Start the transcoder (which will just generate dummy Opus packets)
            Arc::clone(&transcoder).start_transcoding_shared();
            
            // No need to connect to stream manager, but keep for API consistency
            println!("NOTE: Using minimal transcoder that generates dummy Opus packets");
            if transcoder.is_transcoding() {
                println!("Transcoder is running!");
            } else {
                println!("WARNING: Transcoder failed to start");
            }
        }
    } else {
        println!("Not starting broadcast thread - no tracks available");
    }
    
    // Start monitoring thread to handle track switching
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        println!("Starting track monitoring thread...");
        crate::services::playlist::track_switcher(stream_manager_for_monitor.clone());
    });
    
    println!("Server initialization complete, starting web server...");
    
    // Build and launch the Rocket instance
    rocket::build()
        .manage(stream_manager.clone())
        .manage(websocket_bus.clone())
        .manage(transcoder.clone())
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_stats,
            handlers::stream_ws,  // MP3 streaming
            handlers::stream_opus_ws,  // Opus streaming for iOS
            handlers::direct_stream,
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
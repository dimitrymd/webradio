// Updated main.rs with transcoder support

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
mod transcoder;

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
    
    // Initialize the transcoder for iOS
    let mut transcoder = TranscoderManager::new(
        config::OPUS_BUFFER_SIZE,
        config::OPUS_CHUNK_SIZE,
    );
    
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
            println!("Starting MP3 to Opus transcoder...");
            transcoder.start_transcoding();
            
            // Connect the stream manager to feed MP3 data to the transcoder
            println!("Connecting stream manager to transcoder...");
            stream_manager.connect_transcoder(&transcoder);
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
    
    // Create a reference to the transcoder for the handler
    let transcoder_arc = Arc::new(transcoder);
    
    println!("Server initialization complete, starting web server...");
    
    // Build and launch the Rocket instance
    rocket::build()
        .manage(stream_manager.clone())
        .manage(websocket_bus.clone())
        .manage(transcoder_arc.clone())
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_stats,
            handlers::stream_ws,  // MP3 streaming
            handlers::stream_opus_ws,  // Opus streaming for iOS
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
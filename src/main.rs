// src/main.rs - Clean direct streaming only

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
mod direct_stream;

use crate::services::streamer::StreamManager;
use crate::services::playlist;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    // Initialize logging
    env_logger::init();
    
    println!("============================================================");
    println!("Starting Rust MP3 Web Radio (Direct Streaming Edition)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("Chunk size: {} KB", config::CHUNK_SIZE / 1024);
    println!("Features enabled:");
    println!("  ✓ Chunked direct streaming (memory efficient)");
    println!("  ✓ iOS Safari compatibility");
    println!("  ✓ HTTP Range request support");
    println!("  ✓ Position-synchronized playback");
    println!("============================================================");

    // Initialize the stream manager (simplified for direct streaming only)
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Rescan and update track durations before starting
    println!("Checking and updating track durations...");
    playlist::rescan_and_update_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Ensure we have tracks to play
    let has_tracks = if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
        println!("✓ Initial track ready: \"{}\" by {} ({}s)", track.title, track.artist, track.duration);
        true
    } else {
        println!("⚠ WARNING: No tracks available for initial playback");
        
        // Try to scan music folder for new tracks
        println!("Scanning music folder for MP3 files...");
        playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
        
        // Check again after scanning
        if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            println!("✓ Found track after scanning: \"{}\" by {} ({}s)", track.title, track.artist, track.duration);
            true
        } else {
            println!("✗ ERROR: No MP3 files found in music folder");
            println!("Please add MP3 files to: {}", config::MUSIC_FOLDER.display());
            false
        }
    };
    
    // Start the internal track management thread only if we have tracks
    if has_tracks {
        println!("Starting internal track management...");
        stream_manager.start_broadcast_thread();
    } else {
        println!("Skipping track management - no tracks available");
        println!("The server will start but won't stream audio until tracks are added");
    }

    // Start track monitoring and switching thread
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        println!("Starting track monitoring thread...");
        crate::services::playlist::track_switcher(stream_manager_for_monitor);
    });
    
    println!("Server components initialized successfully");
    println!("Starting Rocket web server on http://{}:{}...", config::HOST, config::PORT);
    
    // Build and launch the Rocket instance - clean and minimal
    rocket::build()
        .manage(stream_manager.clone())
        .mount("/", routes![
            // Main web interface
            handlers::index,
            
            // API endpoints
            handlers::now_playing,
            handlers::get_stats,
            
            // Direct streaming endpoints (primary method)
            direct_stream::direct_stream,
            direct_stream::direct_stream_options,
            direct_stream::stream_status,
            
            // Static files and diagnostics
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
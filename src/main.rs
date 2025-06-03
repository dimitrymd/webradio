// src/main.rs - Simplified with proper shutdown

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers, Shutdown};
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
    println!("ChillOut Radio - Simplified Implementation v4.0");
    println!("Single broadcast thread with proper timing");
    println!("============================================================");

    // Initialize stream manager
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Ensure music directory exists
    if !config::MUSIC_FOLDER.exists() {
        std::fs::create_dir_all(&*config::MUSIC_FOLDER).unwrap_or_else(|e| {
            eprintln!("Failed to create music directory: {}", e);
        });
    }
    
    // Scan for music files
    println!("Scanning for MP3 files...");
    playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
    
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    if playlist_data.tracks.is_empty() {
        println!("⚠️  No MP3 files found in music folder");
        println!("   Add MP3 files to: {}", config::MUSIC_FOLDER.display());
    } else {
        println!("✅ Found {} tracks", playlist_data.tracks.len());
        
        if let Some(current_track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            println!("   Starting with: \"{}\" by {}", current_track.title, current_track.artist);
        }
    }

    // Update track durations
    println!("Updating track durations...");
    playlist::rescan_and_update_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Start broadcast thread
    println!("Starting radio broadcast...");
    stream_manager.start_broadcast_thread();
    
    // Set up shutdown handler
    let stream_manager_for_shutdown = stream_manager.clone();
    ctrlc::set_handler(move || {
        println!("\n📻 Shutting down radio broadcast...");
        stream_manager_for_shutdown.stop_broadcasting();
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    
    println!("✅ Radio is broadcasting!");
    println!("🌐 Server at: http://localhost:8000");
    println!("📻 Stream at: http://localhost:8000/direct-stream");
    println!("📊 Status at: http://localhost:8000/stream-status");
    println!("🛑 Press Ctrl+C to stop");
    println!("============================================================");
    
    // Build Rocket server
    rocket::build()
        .manage(stream_manager)
        .mount("/", routes![
            // Streaming endpoints
            direct_stream::direct_stream,
            direct_stream::direct_stream_options,
            direct_stream::stream_status,
            direct_stream::radio_stream,
            
            // API endpoints
            handlers::now_playing,
            handlers::heartbeat,
            handlers::get_stats,
            handlers::get_position,
            handlers::get_playlist,
            handlers::health_check,
            handlers::get_connections,
            
            // Web interface
            handlers::index,
            
            // Static files
            handlers::static_files,
            handlers::diagnostic_page,
            handlers::favicon,
            handlers::robots,
            handlers::cors_preflight,
        ])
        .register("/", catchers![
            handlers::not_found,
            handlers::server_error,
            handlers::service_unavailable,
        ])
        .attach(Template::fairing())
}
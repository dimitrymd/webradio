// src/main.rs - Complete radio implementation

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
    println!("ChillOut Radio - True Radio Broadcasting");
    println!("All listeners synchronized to the same live stream");
    println!("============================================================");

    // Initialize stream manager
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Scan for music files
    println!("Scanning for MP3 files...");
    playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
    
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    if playlist_data.tracks.is_empty() {
        println!("⚠️  No MP3 files found in music folder");
        println!("   Add MP3 files to: {}", config::MUSIC_FOLDER.display());
        println!("   Server will start with demo audio");
    } else {
        println!("✅ Found {} tracks", playlist_data.tracks.len());
        if let Some(current) = playlist_data.tracks.first() {
            println!("   First track: \"{}\" by {}", current.title, current.artist);
        }
    }

    // Start broadcast thread
    println!("Starting radio broadcast...");
    stream_manager.start_broadcast_thread();
    
    // Give it a moment to initialize
    std::thread::sleep(std::time::Duration::from_millis(1000));
    
    if stream_manager.is_streaming() {
        println!("✅ Radio broadcast started successfully");
    } else {
        println!("⚠️  Radio broadcast may not have started properly");
    }

    // Start playlist monitoring thread
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        playlist::track_switcher(stream_manager_for_monitor);
    });
    
    println!("🎵 True Radio Mode: Single broadcast for all listeners");
    println!("🌐 Server starting at: http://localhost:8000");
    println!("============================================================");
    
    // Build Rocket server
    rocket::build()
        .manage(stream_manager)
        .mount("/", routes![
            // Main interface
            handlers::index,
            
            // API endpoints
            handlers::now_playing,
            handlers::heartbeat,
            handlers::get_stats,
            handlers::get_position,
            handlers::get_playlist,
            
            // Streaming endpoints
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
        ])
        .attach(Template::fairing())
}
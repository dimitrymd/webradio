// src/main.rs - Updated with track switching endpoint

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
    println!("ChillOut Radio - True Radio Broadcasting (FIXED v2.0)");
    println!("Fixed track switching and synchronized logging");
    println!("============================================================");

    // Initialize stream manager with fixed implementation
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
        println!("   Server will start with demo audio");
        
        // Create a demo track entry
        let demo_track = crate::models::playlist::Track {
            path: "demo.mp3".to_string(),
            title: "ChillOut Radio Demo".to_string(),
            artist: "Demo Artist".to_string(),
            album: "Demo Album".to_string(),
            duration: 180,
        };
        
        let demo_playlist = crate::models::playlist::Playlist {
            current_track: 0,
            tracks: vec![demo_track],
        };
        
        playlist::save_playlist(&demo_playlist, &config::PLAYLIST_FILE);
        println!("   Created demo playlist for testing");
    } else {
        println!("✅ Found {} tracks", playlist_data.tracks.len());
        if let Some(current) = playlist_data.tracks.first() {
            println!("   First track: \"{}\" by {}", current.title, current.artist);
        }
        
        // Show current track from playlist
        if let Some(current_track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            println!("   Current track (index {}): \"{}\" by {}", 
                     playlist_data.current_track, current_track.title, current_track.artist);
        }
    }

    // Verify track durations before starting
    println!("Verifying track durations...");
    playlist::verify_track_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Start broadcast thread
    println!("Starting radio broadcast...");
    stream_manager.start_broadcast_thread();
    
    // Give it a moment to initialize
    std::thread::sleep(std::time::Duration::from_millis(2000));
    
    if stream_manager.is_streaming() {
        println!("✅ Radio broadcast started successfully");
        let (pos_secs, pos_ms) = stream_manager.get_precise_position();
        println!("   Current position: {}s + {}ms", pos_secs, pos_ms);
        
        // Show what's actually playing
        if let Some(track_info) = stream_manager.get_track_info() {
            if let Ok(track_data) = serde_json::from_str::<serde_json::Value>(&track_info) {
                if let Some(title) = track_data.get("title").and_then(|t| t.as_str()) {
                    println!("   Now broadcasting: \"{}\"", title);
                }
            }
        }
    } else {
        println!("⚠️  Radio broadcast may not have started properly");
        println!("   Check logs for errors");
    }

    // Start playlist monitoring thread
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        playlist::track_switcher(stream_manager_for_monitor);
    });
    
    println!("🎵 True Radio Mode: Single broadcast for all listeners");
    println!("🔧 Fixed Issues v2.0:");
    println!("   ✅ Synchronized track info and logging");
    println!("   ✅ Proper track switching coordination");
    println!("   ✅ Duration-based automatic track switching");
    println!("   ✅ Manual track switch endpoint: /api/switch-track");
    println!("   ✅ Broadcast thread manages track state directly");
    println!("🌐 Server starting at: http://localhost:8000");
    println!("🔍 Diagnostics available at: http://localhost:8000/diag");
    println!("🎚️  Manual track switch: http://localhost:8000/api/switch-track");
    println!("============================================================");
    
    // Build Rocket server with all fixed routes including track switching
    rocket::build()
        .manage(stream_manager)
        .mount("/", routes![
            // Main interface
            handlers::index,
            
            // Core API endpoints
            handlers::now_playing,
            handlers::heartbeat,
            handlers::get_stats,
            handlers::get_position,
            handlers::get_playlist,
            
            // Track control endpoints
            handlers::switch_track,     // NEW: Manual track switching
            handlers::get_next_track,   // NEW: Preview next track
            
            // Additional API endpoints  
            handlers::health_check,
            handlers::get_connections,
            
            // Streaming endpoints
            direct_stream::direct_stream,
            direct_stream::direct_stream_options,
            direct_stream::stream_status,
            direct_stream::radio_stream,
            
            // Static files and utilities
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
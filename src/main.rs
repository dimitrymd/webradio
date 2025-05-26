// src/main.rs - Fixed imports

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
    println!("Starting Rust MP3 Web Radio (Radio Mode - Live Streaming Edition)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("Chunk size: {} KB", config::CHUNK_SIZE / 1024);
    println!("Features enabled:");
    println!("  ✓ Live radio-style streaming (synchronized playback)");
    println!("  ✓ All listeners hear the same thing at the same time");
    println!("  ✓ No seeking - tune in to current radio time");
    println!("  ✓ Enhanced position synchronization (millisecond precision)");
    println!("  ✓ Mobile-optimized radio experience");
    println!("  ✓ Accurate listener count tracking");
    println!("  ✓ Connection heartbeat system");
    println!("  ✓ Cross-platform radio compatibility");
    println!("============================================================");

    // Initialize the enhanced stream manager
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Rescan and update track durations before starting
    println!("Checking and updating track durations...");
    playlist::rescan_and_update_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Verify track durations for debugging
    println!("Verifying track duration accuracy...");
    playlist::verify_track_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Ensure we have tracks to play
    let has_tracks = if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
        println!("✓ Initial track ready: \"{}\" by {} ({}s)", track.title, track.artist, track.duration);
        
        // Validate track duration
        if track.duration == 0 {
            println!("⚠ WARNING: Track has zero duration, position sync may be affected");
        } else if track.duration < 10 {
            println!("⚠ WARNING: Track duration is very short ({}s), may cause rapid transitions", track.duration);
        }
        
        // Additional Android-specific warnings
        if track.duration > 0 {
            println!("✓ Android position sync: Track duration is valid for accurate position calculation");
        }
        
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
        println!("Starting radio broadcast thread with live synchronization...");
        println!("Radio mode: All listeners synchronized to current server time");
        stream_manager.start_broadcast_thread();
        
        // Give the thread a moment to initialize
        std::thread::sleep(std::time::Duration::from_millis(500));
        
        // Verify stream manager is working
        if stream_manager.is_streaming() {
            let (pos_secs, pos_ms) = stream_manager.get_precise_position();
            println!("✓ Stream manager initialized - Position: {}s + {}ms", pos_secs, pos_ms);
            println!("✓ Android clients will receive server-authoritative position data");
        } else {
            println!("⚠ WARNING: Stream manager may not have started properly");
        }
    } else {
        println!("Skipping track management - no tracks available");
        println!("The server will start but won't stream audio until tracks are added");
    }

    // Start track monitoring and switching thread
    let stream_manager_for_monitor = stream_manager.clone();
    thread::spawn(move || {
        println!("Starting enhanced track monitoring thread...");
        crate::services::playlist::track_switcher(stream_manager_for_monitor);
    });
    
    println!("Enhanced server components initialized successfully");
    println!("Position sync accuracy: Millisecond precision with drift correction");
    println!("Android fixes: Server-authoritative position, strict validation, enhanced debugging");
    println!("Starting Rocket web server on http://{}:{}...", config::HOST, config::PORT);
    
    // Build and launch the Rocket instance with enhanced endpoints
    rocket::build()
        .manage(stream_manager.clone())
        .mount("/", routes![
            // Main web interface
            handlers::index,
            
            // Enhanced API endpoints with Android support
            handlers::now_playing,
            handlers::get_stats,
            handlers::get_position,          // Detailed position info
            handlers::android_position,     // Android-specific position endpoint
            handlers::sync_check,            // Client sync verification
            handlers::heartbeat,             // Connection heartbeat
            
            // Direct streaming endpoints (Android-enhanced)
            direct_stream::direct_stream,    // Fixed import
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
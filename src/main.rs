// Modified main.rs to ensure routes are correctly declared

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers};
use std::thread;
use std::sync::Arc;
use std::time::Duration;

mod config;
mod handlers;
mod models;
mod services;
mod utils;

use crate::services::streamer::StreamManager;
use crate::services::websocket_bus::WebSocketBus;
use crate::services::playlist;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    println!("============================================================");
    println!("Starting Rust MP3 Web Radio (Direct Streaming for All Platforms)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("============================================================");

    // Initialize the stream manager with the configuration values
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));     
    
    // Ensure playlist file exists and is up to date
    ensure_playlist_is_prepared(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

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
    
    // Create WebSocket bus (still needed for now playing updates)
    let websocket_bus = Arc::new(WebSocketBus::new(stream_manager.clone()));
    
    // Start WebSocket broadcast loop for now playing info
    println!("Starting WebSocket broadcast loop for track info...");
    websocket_bus.clone().start_broadcast_loop();
    
    // Pre-buffer all tracks for smoother transitions
    if has_tracks {
        println!("Pre-buffering tracks for smoother streaming...");
        pre_buffer_tracks(&stream_manager);
    }
    
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
        crate::services::playlist::track_switcher(stream_manager_for_monitor.clone());
    });
    
    // Start health check thread
    let stream_manager_for_health = stream_manager.clone();
    thread::spawn(move || {
        println!("Starting health check thread...");
        loop {
            // Check for potential issues
            check_stream_health(&stream_manager_for_health);
            
            // Sleep between checks
            thread::sleep(Duration::from_secs(30));
        }
    });
    
    println!("Server initialization complete, starting web server...");
    
    // Build and launch the Rocket instance with updated routes
    rocket::build()
        .manage(stream_manager.clone())
        .manage(websocket_bus.clone())
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_stats,
            handlers::direct_stream,
            handlers::static_files,
            handlers::test_page,
        ])
        .register("/", catchers![
            handlers::not_found,
            handlers::server_error,
            handlers::service_unavailable,
        ])
        .attach(Template::fairing())
}

// Function to ensure playlist is properly prepared
fn ensure_playlist_is_prepared(playlist_file: &std::path::Path, music_folder: &std::path::Path) {
    // First check if we need to scan for MP3 files
    let needs_scan = {
        let playlist = playlist::get_playlist(playlist_file);
        playlist.tracks.is_empty()
    };
    
    if needs_scan {
        println!("No tracks in playlist, scanning music folder...");
        playlist::scan_music_folder(music_folder, playlist_file);
    }
    
    // Verify and update track durations
    println!("Checking and updating track durations...");
    playlist::rescan_and_update_durations(playlist_file, music_folder);
    
    // Verify playlist integrity
    playlist::verify_track_durations(playlist_file, music_folder);
}

// Pre-buffer tracks for smoother transitions
fn pre_buffer_tracks(stream_manager: &Arc<StreamManager>) {
    let playlist = playlist::get_playlist(&config::PLAYLIST_FILE);
    
    if playlist.tracks.len() > 1 {
        // Get the next track (after current one)
        let current_index = playlist.current_track;
        let next_index = (current_index + 1) % playlist.tracks.len();
        
        if let Some(next_track) = playlist.tracks.get(next_index) {
            let track_path = config::MUSIC_FOLDER.join(&next_track.path);
            
            println!("Pre-buffering next track: {} by {}", next_track.title, next_track.artist);
            
            // Pre-buffer the next track
            stream_manager.prefetch_next_track(&track_path);
        }
    }
}

// Health check function to detect and fix issues
fn check_stream_health(stream_manager: &Arc<StreamManager>) {
    // Check if streaming is active
    let is_streaming = stream_manager.is_streaming();
    
    if !is_streaming {
        println!("WARNING: Stream is not active, attempting to restart");
        stream_manager.restart_if_needed();
        return;
    }
    
    // Check if we have enough saved chunks for new clients
    let saved_chunks = stream_manager.get_saved_chunks_count();
    let min_expected_chunks = config::MAX_RECENT_CHUNKS / 2;
    
    if saved_chunks < min_expected_chunks {
        println!("WARNING: Low number of saved chunks ({}), potential streaming issue", saved_chunks);
        
        // Check if track has ended
        if stream_manager.track_ended() {
            println!("Track has ended but next track not started, attempting recovery");
            stream_manager.force_next_track();
        }
    }
    
    // Additional health checks as needed...
    let active_listeners = stream_manager.get_active_listeners();
    let current_bitrate = stream_manager.get_current_bitrate();
    
    println!("Stream health check: active={}, chunks={}, bitrate={}kbps", 
             active_listeners, saved_chunks, current_bitrate/1000);
}
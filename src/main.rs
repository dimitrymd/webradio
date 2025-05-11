// Replace main.rs with this version that properly starts the broadcast thread:

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers};
use std::thread;

mod config;
mod handlers;
mod models;
mod services;
mod utils;

use crate::services::streamer::StreamManager;
use crate::services::playlist;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    println!("============================================================");
    println!("Starting Rust MP3 Web Radio (Single-Thread Architecture)");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("============================================================");

    // Initialize the stream manager 
    let stream_manager = StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
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
    
    // Start the broadcast thread only if we have tracks
    if has_tracks {
        println!("Starting broadcast thread...");
        stream_manager.start_broadcast_thread();
    } else {
        println!("Not starting broadcast thread - no tracks available");
    }

    // Start monitoring thread (simplified - just logs status)
    let stream_manager_clone = stream_manager.clone();
    thread::spawn(move || {
        crate::services::playlist::track_switcher(stream_manager_clone);
    });
    
    println!("Server initialization complete, starting web server...");
    
    // Build and launch the Rocket instance
    rocket::build()
        .manage(stream_manager)
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_stats,
            handlers::stream_ws,  // WebSocket endpoint for real-time streaming
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
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
    println!("Starting Rust MP3 Web Radio in broadcast mode");
    println!("Music folder: {}", config::MUSIC_FOLDER.display());
    println!("============================================================");

    // Initialize the stream manager
    let stream_manager = StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    );

    // Start the first track immediately
    println!("Starting initial playback...");
    if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
        println!("Starting initial track: {}", track.title);
        stream_manager.start_streaming(&track.path);
    } else {
        println!("WARNING: No tracks available for initial playback");
    }

    // Start track switcher in a background thread
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
            handlers::stream_ws,      // WebSocket endpoint for real-time streaming
            handlers::direct_stream,  // Direct streaming endpoint
            handlers::static_files,
        ])
        .register("/", catchers![
            handlers::not_found,
            handlers::server_error,
            handlers::service_unavailable,
        ])
        .attach(Template::fairing())
}
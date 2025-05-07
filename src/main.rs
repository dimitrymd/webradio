#[macro_use]
extern crate rocket;

use rocket::fs::relative;
use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers};
use std::thread;

mod config;
mod handlers;
mod models;
mod services;
mod utils;

use crate::services::streamer::StreamManager;

#[launch]
fn rocket() -> rocket::Rocket<rocket::Build> {
    // Initialize logging
    env_logger::init();

    // Initialize the stream manager
    let stream_manager = StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    );

    // Start track switcher in a background thread
    let stream_manager_clone = stream_manager.clone();
    thread::spawn(move || {
        crate::services::playlist::track_switcher(stream_manager_clone);
    });
    
    // Start streaming the first track automatically
    if let Some(track) = crate::services::playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
        log::info!("Starting automatic streaming with track: {}", track.title);
        stream_manager.start_streaming(&track.path);
    }

    // Build and launch the Rocket instance
    rocket::build()
        .manage(stream_manager)
        .mount("/", routes![
            handlers::index,
            handlers::now_playing,
            handlers::get_playlist,
            handlers::scan_music,
            handlers::shuffle_playlist,
            handlers::play_track,
            handlers::next_track,
            handlers::get_stats,
            handlers::stream_ws,   // WebSocket endpoint
            handlers::stream_http, // HTTP stream endpoint
            handlers::static_files,
        ])
        .register("/", catchers![
            handlers::not_found,
            handlers::server_error,
            handlers::service_unavailable,
        ])
        .attach(Template::fairing())
}
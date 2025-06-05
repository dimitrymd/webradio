// src/main.rs - Async I/O entry point

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{launch, routes, catchers, Config};
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
async fn rocket() -> rocket::Rocket<rocket::Build> {
    // Set up minimal logging
    std::env::set_var("RUST_LOG", "error");
    env_logger::init();
    
    println!("============================================================");
    println!("ChillOut Radio - Async I/O v7.0");
    println!("Using async/await for efficient I/O operations");
    println!("============================================================");

    // Initialize stream manager with async runtime handle
    let stream_manager = Arc::new(StreamManager::new(
        &config::MUSIC_FOLDER,
        config::CHUNK_SIZE,
        config::BUFFER_SIZE,
        config::STREAM_CACHE_TIME,
    ));
    
    // Ensure music directory exists
    if !config::MUSIC_FOLDER.exists() {
        tokio::fs::create_dir_all(&*config::MUSIC_FOLDER).await.unwrap_or_else(|e| {
            eprintln!("Failed to create music directory: {}", e);
        });
    }
    
    // Initial scan (async)
    println!("Scanning for MP3 files...");
    let playlist_data = match playlist::scan_music_folder_async(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE).await {
        Ok(playlist) => playlist,
        Err(e) => {
            eprintln!("Error scanning music folder: {}", e);
            crate::models::playlist::Playlist::default()
        }
    };
    
    if playlist_data.tracks.is_empty() {
        println!("⚠️  No MP3 files found in music folder");
        println!("   Add MP3 files to: {}", config::MUSIC_FOLDER.display());
    } else {
        println!("✅ Found {} tracks", playlist_data.tracks.len());
    }

    // Update durations once at startup (async)
    if let Err(e) = playlist::rescan_and_update_durations_async(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER).await {
        eprintln!("Error updating track durations: {}", e);
    }

    // Start broadcast task (async)
    println!("Starting async radio broadcast...");
    stream_manager.start_broadcast_thread();
    
    // Start monitor task (async)
    let monitor_manager = stream_manager.clone();
    tokio::spawn(async move {
        playlist::track_switcher_async(monitor_manager).await;
    });
    
    // Set up shutdown handler
    let stream_manager_for_shutdown = stream_manager.clone();
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                println!("\n📻 Shutting down...");
                stream_manager_for_shutdown.stop_broadcasting();
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("Unable to listen for shutdown signal: {}", err);
            }
        }
    });
    
    println!("✅ Async radio is broadcasting!");
    println!("🌐 Server at: http://localhost:8000");
    println!("📻 Stream at: http://localhost:8000/direct-stream");
    println!("🛑 Press Ctrl+C to stop");
    println!("============================================================");
    
    // Configure Rocket for async I/O
    let rocket_config = Config {
        port: config::PORT,
        address: std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
        keep_alive: config::KEEP_ALIVE_TIMEOUT,
        workers: 2,              // Can use more workers with async
        max_blocking: 4,         // Slightly more blocking threads
        ident: rocket::config::Ident::none(),
        ip_header: None,
        log_level: rocket::config::LogLevel::Off,
        ..Config::default()
    };
    
    // Build Rocket server
    rocket::custom(rocket_config)
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
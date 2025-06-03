// src/main.rs - CPU-optimized entry point

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
fn rocket() -> rocket::Rocket<rocket::Build> {
    // Set up minimal logging
    std::env::set_var("RUST_LOG", "error");
    env_logger::init();
    
    println!("============================================================");
    println!("ChillOut Radio - Ultra CPU-Optimized v6.0");
    println!("Minimal CPU usage configuration active");
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
    
    // Initial scan
    println!("Scanning for MP3 files...");
    playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
    
    let playlist_data = playlist::get_playlist(&config::PLAYLIST_FILE);
    if playlist_data.tracks.is_empty() {
        println!("⚠️  No MP3 files found in music folder");
        println!("   Add MP3 files to: {}", config::MUSIC_FOLDER.display());
    } else {
        println!("✅ Found {} tracks", playlist_data.tracks.len());
    }

    // Update durations once at startup
    playlist::rescan_and_update_durations(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);

    // Start broadcast thread
    println!("Starting CPU-optimized radio broadcast...");
    stream_manager.start_broadcast_thread();
    
    // Start minimal monitor thread
    let monitor_manager = stream_manager.clone();
    std::thread::Builder::new()
        .name("monitor".to_string())
        .spawn(move || {
            // Set monitor thread to lower priority
            #[cfg(unix)]
            {
                unsafe {
                    libc::nice(15); // Even lower priority than broadcast
                }
            }
            playlist::track_switcher(monitor_manager);
        })
        .expect("Failed to spawn monitor thread");
    
    // Set up shutdown handler
    let stream_manager_for_shutdown = stream_manager.clone();
    ctrlc::set_handler(move || {
        println!("\n📻 Shutting down...");
        stream_manager_for_shutdown.stop_broadcasting();
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    
    println!("✅ CPU-optimized radio is broadcasting!");
    println!("🌐 Server at: http://localhost:8000");
    println!("📻 Stream at: http://localhost:8000/direct-stream");
    println!("🛑 Press Ctrl+C to stop");
    println!("============================================================");
    
    // Configure Rocket for minimal CPU usage
    let rocket_config = Config {
        port: config::PORT,
        address: std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
        keep_alive: config::KEEP_ALIVE_TIMEOUT,
        workers: 1,              // Single worker thread!
        max_blocking: 1,         // Minimal blocking threads
        ident: rocket::config::Ident::none(), // Disable ident header
        ip_header: None,         // No IP header parsing
        log_level: rocket::config::LogLevel::Off, // Disable Rocket logging
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
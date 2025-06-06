// src/main.rs - Fully optimized entry point

extern crate rocket;

use rocket_dyn_templates::Template;
use rocket::{routes, catchers, Config};
use std::sync::Arc;

mod config;
mod handlers;
mod models;  
mod services;
mod utils;
mod direct_stream;

use crate::services::streamer::StreamManager;
use crate::services::playlist;

// Create optimized runtime
fn create_optimized_runtime() -> tokio::runtime::Runtime {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    
    builder
        .worker_threads(2)  // Only 2 worker threads
        .max_blocking_threads(1)
        .thread_name("radio-worker")
        .thread_stack_size(2 * 1024 * 1024); // 2MB stack
    
    // Set thread affinity on Linux
    #[cfg(target_os = "linux")]
    builder.on_thread_start(|| {
        use libc::{cpu_set_t, CPU_SET, CPU_ZERO, sched_setaffinity};
        unsafe {
            let mut cpu_set: cpu_set_t = std::mem::zeroed();
            CPU_ZERO(&mut cpu_set);
            CPU_SET(0, &mut cpu_set); // Pin to CPU 0
            sched_setaffinity(0, std::mem::size_of::<cpu_set_t>(), &cpu_set);
            
            // Set thread priority
            libc::nice(10); // Lower priority for worker threads
        }
    });
    
    builder.enable_all().build().unwrap()
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    // Set up minimal logging
    std::env::set_var("RUST_LOG", "error");
    env_logger::init();
    
    println!("============================================================");
    println!("ChillOut Radio - Fully Optimized v4.0");
    println!("Minimal CPU usage with all optimizations");
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
    
    // Initialize playlist watcher
    playlist::init_playlist_watcher(&config::PLAYLIST_FILE);
    
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
    println!("Starting optimized radio broadcast...");
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
    
    println!("✅ Optimized radio is broadcasting!");
    println!("🌐 Server at: http://localhost:8000");
    println!("📻 Stream at: http://localhost:8000/direct-stream");
    println!("🛑 Press Ctrl+C to stop");
    println!("============================================================");
    
    // Configure Rocket with optimizations
    let rocket_config = Config {
        port: config::PORT,
        address: std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
        keep_alive: config::KEEP_ALIVE_TIMEOUT,
        workers: 2,              // Minimal workers
        max_blocking: 2,         // Minimal blocking threads
        ident: rocket::config::Ident::none(),
        ip_header: None,
        log_level: rocket::config::LogLevel::Off,
        ..Config::default()
    };
    
    // Build Rocket server
    let rocket = rocket::custom(rocket_config)
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
        .attach(Template::fairing());
    
    rocket.launch().await?;
    Ok(())?;
    Ok(())
}
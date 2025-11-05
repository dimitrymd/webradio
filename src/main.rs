use axum::{
    Router,
    extract::State,
    response::{Html, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, get_service},
    http::{StatusCode, header},
    Json,
};
use tower_http::{
    services::ServeDir,
    cors::{CorsLayer, Any},
    trace::TraceLayer,
};
use std::{
    net::{SocketAddr, IpAddr},
    sync::Arc,
    time::Duration,
};
use tracing::info;
use tokio::signal;
use futures::stream::Stream;

mod error;
mod radio;
mod playlist;
mod config;

use error::AppError;
use radio::RadioStation;
use config::Config;

type AppState = Arc<RadioStation>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "webradio=debug,tower_http=info,axum=info".into()),
        )
        .init();

    // Load configuration
    let config = Config::from_env();
    info!("Starting WebRadio v5.0 on {}:{}", config.host, config.port);

    // Create radio station
    let station = Arc::new(RadioStation::new(config.clone()).await?);

    // Start the radio broadcast
    Arc::clone(&station).start_broadcast();

    // Build router
    let app = create_router(station.clone(), &config);

    // Create address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on http://{}", addr);

    // Display all available network interfaces for easier access
    display_network_info(config.port);

    // Run server with graceful shutdown
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(station.clone()));

    server.await?;

    Ok(())
}

fn display_network_info(port: u16) {
    info!("═══════════════════════════════════════════════════");
    info!("🎵 WebRadio is ready! Connect from any device:");
    info!("───────────────────────────────────────────────────");

    // Try to get network interfaces
    if let Ok(interfaces) = get_local_ips() {
        for (name, ip) in interfaces {
            if !ip.is_loopback() {
                info!("  📱 {:<15} → http://{}:{}", name, ip, port);
            }
        }
    }

    info!("  💻 Local           → http://localhost:{}", port);
    info!("───────────────────────────────────────────────────");

    // Try to get external IP
    tokio::spawn(async move {
        if let Ok(external_ip) = get_external_ip().await {
            info!("  🌍 External        → http://{}:{}", external_ip, port);
            info!("═══════════════════════════════════════════════════");
        }
    });
}

fn get_local_ips() -> Result<Vec<(String, IpAddr)>, std::io::Error> {
    let mut ips = Vec::new();

    // Use a simple approach that works across platforms
    if let Ok(hostname) = hostname::get() {
        if let Ok(hostname_str) = hostname.into_string() {
            if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&format!("{}:0", hostname_str)) {
                for addr in addrs {
                    let ip = addr.ip();
                    if ip.is_ipv4() && !ip.is_loopback() {
                        let name = if ip.to_string().starts_with("192.168.") {
                            "WiFi/LAN"
                        } else if ip.to_string().starts_with("10.") {
                            "Private"
                        } else {
                            "Network"
                        };
                        ips.push((name.to_string(), ip));
                    }
                }
            }
        }
    }

    // Alternative method: try common interface names
    if ips.is_empty() {
        // Try to parse from system commands (platform-specific fallback)
        #[cfg(unix)]
        {
            if let Ok(output) = std::process::Command::new("hostname")
                .arg("-I")
                .output()
            {
                if let Ok(ips_str) = String::from_utf8(output.stdout) {
                    for ip_str in ips_str.split_whitespace() {
                        if let Ok(ip) = ip_str.parse::<IpAddr>() {
                            if ip.is_ipv4() && !ip.is_loopback() {
                                ips.push(("Network".to_string(), ip));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(ips)
}

async fn get_external_ip() -> Result<String, Box<dyn std::error::Error>> {
    // Try multiple services for reliability
    let services = [
        "https://api.ipify.org",
        "https://ipinfo.io/ip",
        "https://checkip.amazonaws.com",
    ];

    for service in &services {
        if let Ok(response) = tokio::time::timeout(
            Duration::from_secs(2),
            reqwest::get(*service)
        ).await {
            if let Ok(resp) = response {
                if let Ok(text) = resp.text().await {
                    return Ok(text.trim().to_string());
                }
            }
        }
    }

    Err("Could not determine external IP".into())
}

fn create_router(state: AppState, _config: &Config) -> Router {
    Router::new()
        // Main routes
        .route("/", get(index))
        .route("/stream", get(audio_stream))
        .route("/test-audio", get(test_audio))
        .route("/events", get(sse_events))
        
        // API routes
        .route("/api/now-playing", get(now_playing))
        .route("/api/listeners", get(listener_count))
        .route("/api/playlist", get(get_playlist))
        .route("/api/stats", get(get_stats))
        .route("/api/health", get(health_check))
        .route("/api/debug", get(debug_info))
        
        // Static files
        .nest_service(
            "/static",
            get_service(ServeDir::new("static"))
                .handle_error(|_| async { StatusCode::NOT_FOUND }),
        )
        
        // Add middleware
        .layer(CorsLayer::new().allow_origin(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal(station: AppState) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received CTRL+C signal, initiating graceful shutdown");
        },
        _ = terminate => {
            info!("Received terminate signal, initiating graceful shutdown");
        },
    }

    // Stop the broadcast explicitly
    station.stop_broadcast().await;

    // Force exit after a short grace period
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        info!("Forcing exit...");
        std::process::exit(0);
    });
}

// Route handlers

async fn index() -> Html<&'static str> {
    Html(include_str!("../templates/index.html"))
}

async fn audio_stream(
    State(station): State<AppState>,
    headers: axum::http::HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, AppError> {
    // Log request details to debug multiple connections
    let user_agent = headers.get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let range = headers.get("range")
        .and_then(|v| v.to_str().ok());

    // Check client type from query parameter
    let client_type = query.get("type").map(|s| s.as_str()).unwrap_or("unknown");
    let is_ios = client_type == "ios" || user_agent.contains("iPhone") || user_agent.contains("iPad");

    // Check if this is Safari doing its probe
    let is_safari = user_agent.contains("Safari") && !user_agent.contains("Chrome");

    info!("New audio stream request from: {} (type: {}, range: {:?}, safari: {}, ios: {})",
        user_agent, client_type, range, is_safari, is_ios);

    // For range requests from Safari, we need to handle them specially
    // Safari won't play the stream unless we respond to its range probe
    if let Some(range_header) = range {
        if range_header == "bytes=0-1" {
            // Safari's initial probe - send a small response
            info!("Handling Safari probe request");
            return Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, "audio/mpeg")
                .header("Content-Range", "bytes 0-1/999999999")
                .header("Accept-Ranges", "bytes")
                .header(header::CONTENT_LENGTH, "2")
                .body(axum::body::Body::from(vec![0xFF, 0xFB]))?);  // MP3 sync bytes
        }
        // For other range requests, just stream normally
        info!("Converting range request to normal stream");
    }

    let stream = station.create_audio_stream(is_ios).await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .header(header::CONNECTION, "close")
        .header("X-Content-Type-Options", "nosniff")
        .header("Accept-Ranges", "none")
        .header("Transfer-Encoding", "chunked")
        .body(axum::body::Body::from_stream(stream))?)
}

async fn test_audio() -> Result<Response, AppError> {
    info!("Test audio request");
    
    // Generate a simple sine wave as MP3-like data for testing
    let test_data = vec![0xFF, 0xFB, 0x90, 0x00]; // MP3 frame header
    let mut audio_data = test_data;
    
    // Add some data
    for _ in 0..1000 {
        audio_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    }
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CONTENT_LENGTH, audio_data.len().to_string())
        .body(axum::body::Body::from(audio_data))?)
}

async fn sse_events(
    State(station): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, AppError>>> {
    let stream = station.create_event_stream();
    
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
}

async fn now_playing(
    State(station): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let info = station.get_now_playing();
    Ok(Json(info))
}

async fn listener_count(
    State(station): State<AppState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "listeners": station.listener_count(),
        "uptime": station.uptime_seconds(),
    }))
}

async fn get_playlist(
    State(station): State<AppState>,
) -> Result<Json<playlist::Playlist>, AppError> {
    let playlist = station.get_playlist()?;
    Ok(Json(playlist))
}

async fn get_stats(
    State(station): State<AppState>,
) -> Json<serde_json::Value> {
    Json(station.get_statistics())
}

async fn health_check(
    State(station): State<AppState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "is_broadcasting": station.is_broadcasting(),
        "listeners": station.listener_count(),
        "uptime": station.uptime_seconds(),
    }))
}

async fn debug_info(
    State(station): State<AppState>,
) -> Json<serde_json::Value> {
    let now_playing = station.get_now_playing();
    let stats = station.get_statistics();
    
    Json(serde_json::json!({
        "debug": {
            "is_broadcasting": station.is_broadcasting(),
            "broadcast_receiver_count": station.get_broadcast_receiver_count().await,
            "listener_count": station.listener_count(),
            "now_playing": now_playing,
            "stats": stats,
        }
    }))
}
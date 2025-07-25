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
    net::SocketAddr,
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
    station.start_broadcast();

    // Build router
    let app = create_router(station.clone(), &config);

    // Create address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on http://{}", addr);

    // Run server with graceful shutdown
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(station.clone()));
    
    // Handle CTRL+C in a separate task
    let station_for_shutdown = station.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C handler");
        info!("CTRL+C received, initiating shutdown...");
        station_for_shutdown.stop_broadcast().await;
        // Give a moment for cleanup
        tokio::time::sleep(Duration::from_millis(100)).await;
        std::process::exit(0);
    });
    
    server.await?;

    Ok(())
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
            info!("Received CTRL+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }

    info!("Shutdown signal received, stopping broadcast...");
    station.stop_broadcast().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}

// Route handlers

async fn index() -> Html<&'static str> {
    Html(include_str!("../templates/index.html"))
}

async fn audio_stream(
    State(station): State<AppState>,
) -> Result<Response, AppError> {
    info!("New audio stream request");
    
    let stream = station.create_audio_stream().await?;
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .header(header::CONNECTION, "close")
        .header("X-Content-Type-Options", "nosniff")
        .header("Accept-Ranges", "none")
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
            "broadcast_receiver_count": station.get_broadcast_receiver_count(),
            "listener_count": station.listener_count(),
            "now_playing": now_playing,
            "stats": stats,
        }
    }))
}
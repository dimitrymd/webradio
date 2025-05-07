use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, post, catch};
use rocket_dyn_templates::{Template, context};
use rocket_ws as ws;
use rocket::futures::SinkExt; // For WebSocket streaming
use std::path::{Path, PathBuf};
use std::io::Cursor;
use rocket::response::Response;

use crate::models::playlist::Playlist;
use crate::services::playlist;
use crate::services::streamer::StreamManager;
use crate::config;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {
        title: "MP3 Web Radio",
    })
}

#[get("/api/now-playing")]
pub async fn now_playing(stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    match track {
        Some(track) => {
            // Add active listener count
            let mut track_json = serde_json::to_value(track).unwrap_or_default();
            if let serde_json::Value::Object(ref mut map) = track_json {
                map.insert(
                    "active_listeners".to_string(), 
                    serde_json::Value::Number(serde_json::Number::from(stream_manager.get_active_listeners()))
                );
            }
            
            Json(track_json)
        },
        None => Json(serde_json::json!({
            "error": "No tracks available"
        }))
    }
}

#[get("/api/playlist")]
pub async fn get_playlist() -> Json<Playlist> {
    let playlist = playlist::get_playlist(&config::PLAYLIST_FILE);
    Json(playlist)
}

#[post("/api/playlist/scan")]
pub async fn scan_music() -> Json<serde_json::Value> {
    let playlist = playlist::scan_music_folder(&config::MUSIC_FOLDER, &config::PLAYLIST_FILE);
    
    Json(serde_json::json!({
        "message": format!("Found {} tracks", playlist.tracks.len()),
        "tracks": playlist.tracks.len()
    }))
}

#[post("/api/playlist/shuffle")]
pub async fn shuffle_playlist() -> Json<serde_json::Value> {
    let playlist = playlist::shuffle_playlist(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    Json(serde_json::json!({
        "message": "Playlist shuffled",
        "tracks": playlist.tracks.len()
    }))
}

#[post("/api/playlist/play/<index>")]
pub async fn play_track(index: usize, stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    let mut playlist = playlist::get_playlist(&config::PLAYLIST_FILE);
    
    if playlist.tracks.is_empty() {
        return Json(serde_json::json!({
            "error": "No tracks available"
        }));
    }
    
    if index >= playlist.tracks.len() {
        return Json(serde_json::json!({
            "error": "Invalid track index"
        }));
    }
    
    playlist.current_track = index;
    playlist::save_playlist(&playlist, &config::PLAYLIST_FILE);
    
    // Start streaming new track
    let track = &playlist.tracks[index];
    stream_manager.start_streaming(&track.path);
    
    Json(serde_json::json!({
        "status": "ok"
    }))
}

#[post("/api/next")]
pub async fn next_track(stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    let track = playlist::advance_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    
    if let Some(track) = track {
        stream_manager.start_streaming(&track.path);
    }
    
    Json(serde_json::json!({
        "status": "ok"
    }))
}

#[get("/api/stats")]
pub async fn get_stats(stream_manager: &State<StreamManager>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "active_listeners": stream_manager.get_active_listeners(),
        "max_concurrent_users": config::MAX_CONCURRENT_USERS
    }))
}

// WebSocket handler for streaming audio
#[get("/stream")]
pub fn stream_ws(ws: ws::WebSocket, stream_manager: &State<StreamManager>) -> ws::Channel<'static> {
    log::info!("WebSocket connection request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        // Return an error message and close the connection
        log::warn!("Max concurrent users limit reached");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("Server at capacity, try again later".into())).await;
            Ok(())
        }));
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
        // Return an error message and close the connection
        log::warn!("No tracks available");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("No tracks available".into())).await;
            Ok(())
        }));
    }
    
    // We don't need to start streaming here anymore since it's already running
    // from the server startup. Just get the current track details.
    log::info!("Creating WebSocket channel for client");
    
    // Clone the stream manager for use in the closure
    let stream_manager = stream_manager.inner().clone();
    
    // Create a WebSocket channel
    ws.channel(move |mut stream| Box::pin(async move {
        // Send track info first
        if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            let track_info = serde_json::to_string(&track).unwrap_or_default();
            log::info!("Sending track info: {}", track_info);
            let _ = stream.send(ws::Message::Text(track_info)).await;
        }
        
        // Get stream generator and start sending audio chunks
        let mut audio_stream = stream_manager.get_stream_generator();
        
        // Increment active listener count
        stream_manager.increment_listener_count();
        
        // Send audio chunks over WebSocket
        let mut chunk_count = 0;
        while let Some(chunk) = audio_stream.next() {
            chunk_count += 1;
            if chunk_count % 10 == 0 {
                log::info!("Sent {} audio chunks", chunk_count);
            }
            
            // Send binary data
            let result = stream.send(ws::Message::Binary(chunk.clone())).await;
            
            // If sending fails, break the loop
            if result.is_err() {
                log::error!("Error sending audio chunk: {:?}", result.err());
                break;
            }
            
            // Sleep a bit to control streaming rate
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        log::info!("WebSocket stream ended after sending {} chunks", chunk_count);
        
        // Decrement active listener count when done
        stream_manager.decrement_listener_count();
        
        Ok(())
    }))
}

// HTTP streaming endpoint for standard audio players
#[get("/stream.mp3")]
pub async fn stream_http(stream_manager: &State<StreamManager>) -> Result<Response<'static>, Status> {
    log::info!("HTTP stream request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        log::warn!("Max concurrent users limit reached");
        return Err(Status::ServiceUnavailable);
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
        log::warn!("No tracks available");
        return Err(Status::NotFound);
    }
    
    // Increment listener count
    stream_manager.increment_listener_count();
    
    // Stream the file directly if it exists
    if let Some(track) = track {
        let file_path = config::MUSIC_FOLDER.join(&track.path);
        
        // Read the file
        match tokio::fs::read(&file_path).await {
            Ok(data) => {
                log::info!("Started HTTP streaming of file: {}", file_path.display());
                
                // Create a cursor that implements AsyncRead
                let cursor = Cursor::new(data);
                
                // Build the response with MP3 content type
                let mut response = Response::build()
                    .header(ContentType::MP3)
                    .finalize();
                
                // We're using the standard method for setting sized bodies in Rocket 0.5
                response.set_streamed_body(cursor);
                
                Ok(response)
            },
            Err(e) => {
                log::error!("Failed to read file {}: {}", file_path.display(), e);
                stream_manager.decrement_listener_count();
                Err(Status::InternalServerError)
            }
        }
    } else {
        stream_manager.decrement_listener_count();
        Err(Status::NotFound)
    }
}

// Helper function to serve static files
#[get("/static/<file..>")]
pub async fn static_files(file: PathBuf) -> Option<NamedFile> {
    let path = Path::new("static/").join(file);
    NamedFile::open(path).await.ok()
}

// Error catchers
#[catch(404)]
pub async fn not_found() -> Template {
    Template::render("error", context! {
        status: 404,
        message: "Page not found"
    })
}

#[catch(500)]
pub async fn server_error() -> Template {
    Template::render("error", context! {
        status: 500,
        message: "Internal server error"
    })
}

#[catch(503)]
pub async fn service_unavailable() -> Template {
    Template::render("error", context! {
        status: 503,
        message: "Server at capacity, try again later"
    })
}
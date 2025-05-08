use rocket::http::{ContentType, Status};
use rocket::State;
use rocket::serde::json::Json;
use rocket::fs::NamedFile;
use rocket::{get, catch};
use rocket_dyn_templates::{Template, context};
use rocket_ws as ws;
use rocket::futures::SinkExt; // For WebSocket streaming
use std::path::{Path, PathBuf};

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
    println!("WebSocket connection request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        // Return an error message and close the connection
        println!("Max concurrent users limit reached");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("Server at capacity, try again later".into())).await;
            Ok(())
        }));
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
        // Return an error message and close the connection
        println!("No tracks available");
        return ws.channel(move |mut stream| Box::pin(async move {
            let _ = stream.send(ws::Message::Text("No tracks available".into())).await;
            Ok(())
        }));
    }
    
    // Clone the stream manager for use in the closure
    let stream_manager = stream_manager.inner().clone();
    
    // Create a WebSocket channel
    ws.channel(move |mut stream| Box::pin(async move {
        // Send track info first
        if let Some(track) = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER) {
            let track_info = serde_json::to_string(&track).unwrap_or_default();
            println!("Sending track info to client: {}", track_info);
            if let Err(e) = stream.send(ws::Message::Text(track_info)).await {
                println!("Error sending track info: {:?}", e);
                return Ok(());
            }
        }
        
        // Get stream generator and start sending audio chunks
        let mut audio_stream = stream_manager.get_stream_generator();
        
        // Increment active listener count
        stream_manager.increment_listener_count();
        println!("Listener connected. Active listeners: {}", stream_manager.get_active_listeners());
        
        // Send audio chunks over WebSocket
        let mut chunk_count = 0;
        let mut error_count = 0;
        
        while let Some(chunk) = audio_stream.next() {
            chunk_count += 1;
            if chunk_count % 100 == 0 {
                println!("Sent {} audio chunks to client", chunk_count);
            }
            
            // Send binary data
            match stream.send(ws::Message::Binary(chunk.clone())).await {
                Ok(_) => {
                    // Successfully sent chunk, reset error count
                    error_count = 0;
                },
                Err(e) => {
                    // Error sending chunk
                    error_count += 1;
                    println!("Error sending audio chunk: {:?}", e);
                    
                    if error_count >= 5 {
                        println!("Too many errors, closing WebSocket connection");
                        break;
                    }
                    
                    // Brief pause before trying again
                    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                }
            }
            
            // Sleep a bit to control streaming rate
            // Using a shorter sleep time for better responsiveness
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
        
        println!("WebSocket stream ended after sending {} chunks", chunk_count);
        
        // Decrement active listener count when done
        stream_manager.decrement_listener_count();
        println!("Listener disconnected. Active listeners: {}", stream_manager.get_active_listeners());
        
        Ok(())
    }))
}

// Direct streaming endpoint - serves the current MP3 file
#[get("/direct-stream")]
pub async fn direct_stream(stream_manager: &State<StreamManager>) -> Result<(ContentType, Vec<u8>), Status> {
    println!("Direct stream request received");
    
    // Check if max concurrent users limit is reached
    if stream_manager.get_active_listeners() >= config::MAX_CONCURRENT_USERS {
        println!("Max concurrent users limit reached");
        return Err(Status::ServiceUnavailable);
    }
    
    let track = playlist::get_current_track(&config::PLAYLIST_FILE, &config::MUSIC_FOLDER);
    if track.is_none() {
        println!("No tracks available for direct streaming");
        return Err(Status::NotFound);
    }
    
    // Increment listener count
    stream_manager.increment_listener_count();
    
    // Stream the file directly if it exists
    if let Some(track) = track {
        let file_path = config::MUSIC_FOLDER.join(&track.path);
        println!("Direct stream request for track: \"{}\" by \"{}\"", track.title, track.artist);
        
        // Read the file
        match tokio::fs::read(&file_path).await {
            Ok(data) => {
                println!("Started direct streaming of file: {} ({} bytes)", file_path.display(), data.len());
                
                // Return the MP3 data with the correct content type
                Ok((ContentType::MP3, data))
            },
            Err(e) => {
                println!("Failed to read file {}: {}", file_path.display(), e);
                stream_manager.decrement_listener_count();
                Err(Status::InternalServerError)
            }
        }
    } else {
        stream_manager.decrement_listener_count();
        println!("Failed to get track for direct streaming");
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
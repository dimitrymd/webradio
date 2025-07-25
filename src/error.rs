use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("HTTP error: {0}")]
    Http(#[from] axum::http::Error),
    
    #[error("Not found")]
    NotFound,
    
    #[error("Internal server error")]
    Internal,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found"),
            AppError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IO error"),
            AppError::Serialization(_) => (StatusCode::BAD_REQUEST, "Invalid data"),
            AppError::Http(_) => (StatusCode::INTERNAL_SERVER_ERROR, "HTTP error"),
            AppError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
        };
        
        (status, message).into_response()
    }
}
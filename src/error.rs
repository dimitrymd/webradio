use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Error, Debug)]
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_not_found() {
        let error = AppError::NotFound;
        assert_eq!(error.to_string(), "Not found");
    }

    #[test]
    fn test_error_internal() {
        let error = AppError::Internal;
        assert_eq!(error.to_string(), "Internal server error");
    }

    #[test]
    fn test_error_from_io() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error = AppError::from(io_error);

        assert!(error.to_string().contains("IO error"));
        assert!(error.to_string().contains("file not found"));
    }

    #[test]
    fn test_error_from_serde() {
        let json_result: std::result::Result<serde_json::Value, serde_json::Error> =
            serde_json::from_str("{invalid json}");

        if let Err(serde_error) = json_result {
            let error = AppError::from(serde_error);
            assert!(error.to_string().contains("Serialization error"));
        } else {
            panic!("Expected serde error");
        }
    }

    #[test]
    fn test_error_response_status_codes() {
        // Test NotFound
        let error = AppError::NotFound;
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test Internal
        let error = AppError::Internal;
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Test IO error
        let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let error = AppError::from(io_error);
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Test Serialization error
        let json_result: std::result::Result<serde_json::Value, serde_json::Error> =
            serde_json::from_str("{bad}");
        if let Err(serde_error) = json_result {
            let error = AppError::from(serde_error);
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[test]
    fn test_result_type_alias() {
        // Test that Result<T> is properly aliased
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        fn returns_error() -> Result<i32> {
            Err(AppError::NotFound)
        }

        assert!(returns_result().is_ok());
        assert_eq!(returns_result().unwrap(), 42);

        assert!(returns_error().is_err());
        match returns_error() {
            Err(AppError::NotFound) => {},
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_error_debug_format() {
        let error = AppError::NotFound;
        let debug_string = format!("{:?}", error);
        assert_eq!(debug_string, "NotFound");

        let error = AppError::Internal;
        let debug_string = format!("{:?}", error);
        assert_eq!(debug_string, "Internal");
    }

    #[test]
    fn test_error_display_with_io_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unexpected end of file");
        let error = AppError::from(io_error);

        let display_string = format!("{}", error);
        assert!(display_string.starts_with("IO error:"));
        assert!(display_string.contains("unexpected end of file"));
    }

    #[test]
    fn test_multiple_error_conversions() {
        // Test that automatic conversions work through the From trait
        let io_error = std::io::Error::new(std::io::ErrorKind::Other, "test error");
        let _app_error: AppError = io_error.into();

        let json_err: std::result::Result<(), serde_json::Error> =
            serde_json::from_str::<()>("invalid");
        if let Err(e) = json_err {
            let _app_error: AppError = e.into();
        }
    }
}
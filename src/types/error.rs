//! 统一的错误类型定义

use std::io;
use serde_json;
use serde_yaml;
use thiserror::Error;

// Axum response support
#[cfg(feature = "api")]
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// 统一的错误类型
#[derive(Error, Debug)]
pub enum PodflowError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    
    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML serialization/deserialization error
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Parse error
    #[error("Parse error: {0}")]
    JsonError(String),

    /// Request error
    #[error("Request error: {0}")]
    RequestError(String),

    /// Lock error
    #[error("Lock error: {0}")]
    LockError(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Not found error
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthError(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionError(String),

    /// Data error
    #[error("Data error: {0}")]
    DataError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Unknown error
    #[error("Unknown error: {0}")]
    UnknownError(String),

    /// Custom error
    #[error("{0}")]
    Custom(String),
}

impl PodflowError {
    /// Create request error
    pub fn request_error(msg: &str) -> Self {
        PodflowError::RequestError(msg.to_string())
    }
    
    /// Create lock error
    pub fn lock_error(msg: &str) -> Self {
        PodflowError::LockError(msg.to_string())
    }
    
    /// Create network error
    pub fn network(msg: &str) -> Self {
        PodflowError::NetworkError(msg.to_string())
    }
    
    /// Create config error
    pub fn config(msg: &str) -> Self {
        PodflowError::ConfigError(msg.to_string())
    }
    
    /// Create validation error
    pub fn validation(msg: &str) -> Self {
        PodflowError::Validation(msg.to_string())
    }
    
    /// Create not-found error
    pub fn not_found(msg: &str) -> Self {
        PodflowError::NotFound(msg.to_string())
    }
    
    /// Create internal error
    pub fn internal(msg: &str) -> Self {
        PodflowError::InternalError(msg.to_string())
    }
    
    /// Create custom error
    pub fn custom(msg: &str) -> Self {
        PodflowError::Custom(msg.to_string())
    }
}

/// Result type alias
pub type Result<T> = std::result::Result<T, PodflowError>;

#[cfg(feature = "api")]
impl IntoResponse for PodflowError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            PodflowError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            PodflowError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            PodflowError::ConfigError(msg) => (StatusCode::BAD_REQUEST, msg),
            PodflowError::NetworkError(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            PodflowError::LockError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PodflowError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            PodflowError::Json(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            PodflowError::Yaml(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            PodflowError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PodflowError::Custom(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            // Handle other variants
            PodflowError::RequestError(msg) => (StatusCode::BAD_REQUEST, msg),
            PodflowError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg),
            PodflowError::AuthError(msg) => (StatusCode::UNAUTHORIZED, msg),
            PodflowError::PermissionError(msg) => (StatusCode::FORBIDDEN, msg),
            PodflowError::DataError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PodflowError::UnknownError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PodflowError::IoError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PodflowError::JsonError(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        let body = Json(serde_json::json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}
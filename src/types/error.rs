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
pub enum NutsError {
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

impl NutsError {
    /// Create request error
    pub fn request_error(msg: &str) -> Self {
        NutsError::RequestError(msg.to_string())
    }
    
    /// Create lock error
    pub fn lock_error(msg: &str) -> Self {
        NutsError::LockError(msg.to_string())
    }
    
    /// Create network error
    pub fn network(msg: &str) -> Self {
        NutsError::NetworkError(msg.to_string())
    }
    
    /// Create config error
    pub fn config(msg: &str) -> Self {
        NutsError::ConfigError(msg.to_string())
    }
    
    /// Create validation error
    pub fn validation(msg: &str) -> Self {
        NutsError::Validation(msg.to_string())
    }
    
    /// Create not-found error
    pub fn not_found(msg: &str) -> Self {
        NutsError::NotFound(msg.to_string())
    }
    
    /// Create internal error
    pub fn internal(msg: &str) -> Self {
        NutsError::InternalError(msg.to_string())
    }
    
    /// Create custom error
    pub fn custom(msg: &str) -> Self {
        NutsError::Custom(msg.to_string())
    }
}

/// Result type alias
pub type Result<T> = std::result::Result<T, NutsError>;

#[cfg(feature = "api")]
impl IntoResponse for NutsError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            NutsError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            NutsError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            NutsError::ConfigError(msg) => (StatusCode::BAD_REQUEST, msg),
            NutsError::NetworkError(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            NutsError::LockError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NutsError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            NutsError::Json(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            NutsError::Yaml(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            NutsError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NutsError::Custom(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            // Handle other variants
            NutsError::RequestError(msg) => (StatusCode::BAD_REQUEST, msg),
            NutsError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg),
            NutsError::AuthError(msg) => (StatusCode::UNAUTHORIZED, msg),
            NutsError::PermissionError(msg) => (StatusCode::FORBIDDEN, msg),
            NutsError::DataError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NutsError::UnknownError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NutsError::IoError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            NutsError::JsonError(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        let body = Json(serde_json::json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}
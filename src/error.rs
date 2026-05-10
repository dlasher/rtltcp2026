use std::fmt;

/// Custom error type for RtlTcp errors
#[allow(dead_code)]
#[derive(Debug)]
pub enum RtlTcpError {
    /// Device-related errors
    DeviceError(String),
    /// Network-related errors
    NetworkError(String),
    /// Configuration errors
    ConfigError(String),
    /// Validation errors
    ValidationError(String),
    /// I/O errors
    IoError(std::io::Error),
}

impl fmt::Display for RtlTcpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RtlTcpError::DeviceError(msg) => write!(f, "Device error: {}", msg),
            RtlTcpError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            RtlTcpError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            RtlTcpError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            RtlTcpError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for RtlTcpError {}

impl From<std::io::Error> for RtlTcpError {
    fn from(error: std::io::Error) -> Self {
        RtlTcpError::IoError(error)
    }
}

impl From<Box<dyn std::error::Error>> for RtlTcpError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        RtlTcpError::DeviceError(error.to_string())
    }
}
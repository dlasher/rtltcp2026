use std::fmt;

/// Custom error type for RtlTcp errors
#[derive(Debug)]
pub enum RtlTcpError {
    /// Device-related errors
    Device(String),
    /// Network-related errors
    Network(String),
    /// Configuration errors
    Config(String),
    /// I/O errors
    Io(std::io::Error),
}

impl fmt::Display for RtlTcpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RtlTcpError::Device(msg) => write!(f, "Device error: {}", msg),
            RtlTcpError::Network(msg) => write!(f, "Network error: {}", msg),
            RtlTcpError::Config(msg) => write!(f, "Configuration error: {}", msg),
            RtlTcpError::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for RtlTcpError {}

impl From<std::io::Error> for RtlTcpError {
    fn from(error: std::io::Error) -> Self {
        RtlTcpError::Io(error)
    }
}

impl From<Box<dyn std::error::Error>> for RtlTcpError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        RtlTcpError::Device(error.to_string())
    }
}

//! Network Error Types
//!
//! Error types for network and transport operations.

use thiserror::Error;

/// Network and transport error types.
#[derive(Error, Debug, Clone)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Connection timeout")]
    Timeout,

    #[error("Message send failed: {0}")]
    SendFailed(String),

    #[error("Message receive failed: {0}")]
    ReceiveFailed(String),

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Relay rejected message: {0}")]
    RelayRejected(String),

    #[error("No acknowledgment received")]
    NoAcknowledgment,

    #[error("Duplicate message: {0}")]
    DuplicateMessage(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Transport not connected")]
    NotConnected,

    #[error("Max retries exceeded")]
    MaxRetriesExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let errors = vec![
            (NetworkError::ConnectionFailed("refused".into()), "Connection failed: refused"),
            (NetworkError::ConnectionClosed, "Connection closed"),
            (NetworkError::Timeout, "Connection timeout"),
            (NetworkError::NotConnected, "Transport not connected"),
            (NetworkError::MaxRetriesExceeded, "Max retries exceeded"),
        ];

        for (error, expected) in errors {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn test_error_clone() {
        let error = NetworkError::ConnectionFailed("test".into());
        let cloned = error.clone();
        assert_eq!(error.to_string(), cloned.to_string());
    }
}

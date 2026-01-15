//! API Error Types
//!
//! Unified error type for the WebBook API layer.

use thiserror::Error;

use crate::contact_card::ValidationError;
use crate::exchange::ExchangeError;
use crate::network::NetworkError;
use crate::storage::StorageError;
use crate::sync::SyncError;

/// Unified error type for WebBook operations.
#[derive(Error, Debug)]
pub enum WebBookError {
    /// Contact card validation failed.
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Key exchange failed.
    #[error("exchange error: {0}")]
    Exchange(#[from] ExchangeError),

    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// Sync operation failed.
    #[error("sync error: {0}")]
    Sync(#[from] SyncError),

    /// Network operation failed.
    #[error("network error: {0}")]
    Network(#[from] NetworkError),

    /// Contact not found.
    #[error("contact not found: {0}")]
    ContactNotFound(String),

    /// Identity not initialized.
    #[error("identity not initialized")]
    IdentityNotInitialized,

    /// Already initialized.
    #[error("already initialized")]
    AlreadyInitialized,

    /// Invalid operation in current state.
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Configuration(String),
}

/// Result type for WebBook operations.
pub type WebBookResult<T> = Result<T, WebBookError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = WebBookError::ContactNotFound("test-id".into());
        assert!(err.to_string().contains("contact not found"));
        assert!(err.to_string().contains("test-id"));
    }

    #[test]
    fn test_error_from_validation() {
        let validation_err = ValidationError::InvalidEmail;
        let err: WebBookError = validation_err.into();
        assert!(matches!(err, WebBookError::Validation(_)));
    }

    #[test]
    fn test_error_from_storage() {
        let storage_err = StorageError::NotFound("key".into());
        let err: WebBookError = storage_err.into();
        assert!(matches!(err, WebBookError::Storage(_)));
    }

    #[test]
    fn test_error_from_network() {
        let network_err = NetworkError::NotConnected;
        let err: WebBookError = network_err.into();
        assert!(matches!(err, WebBookError::Network(_)));
    }

    #[test]
    fn test_error_from_sync() {
        let sync_err = SyncError::NoChanges;
        let err: WebBookError = sync_err.into();
        assert!(matches!(err, WebBookError::Sync(_)));
    }
}

//! Tests for api::error
//! Extracted from error.rs

use webbook_core::api::*;
use webbook_core::*;

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

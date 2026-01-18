//! Tests for api::error
//! Extracted from error.rs

use vauchi_core::api::*;
use vauchi_core::*;

#[test]
fn test_error_display() {
    let err = VauchiError::ContactNotFound("test-id".into());
    assert!(err.to_string().contains("contact not found"));
    assert!(err.to_string().contains("test-id"));
}

#[test]
fn test_error_from_validation() {
    let validation_err = ValidationError::InvalidEmail;
    let err: VauchiError = validation_err.into();
    assert!(matches!(err, VauchiError::Validation(_)));
}

#[test]
fn test_error_from_storage() {
    let storage_err = StorageError::NotFound("key".into());
    let err: VauchiError = storage_err.into();
    assert!(matches!(err, VauchiError::Storage(_)));
}

#[test]
fn test_error_from_network() {
    let network_err = NetworkError::NotConnected;
    let err: VauchiError = network_err.into();
    assert!(matches!(err, VauchiError::Network(_)));
}

#[test]
fn test_error_from_sync() {
    let sync_err = SyncError::NoChanges;
    let err: VauchiError = sync_err.into();
    assert!(matches!(err, VauchiError::Sync(_)));
}

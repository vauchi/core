// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for mobile error type conversions (error.rs).

use vauchi_core::{StorageError, VauchiError};
use vauchi_platform::MobileError;

#[test]
fn test_storage_error_conversion() {
    let storage_err = StorageError::NotFound("test record".to_string());
    let mobile_err: MobileError = storage_err.into();

    let msg = format!("{mobile_err}");
    assert!(
        msg.contains("Storage error"),
        "Should be StorageError variant, got: {msg}"
    );
    // F9 audit fix: user_message() strips internal details — only generic message survives
    assert!(
        msg.contains("storage error occurred"),
        "Should use sanitized user message, got: {msg}"
    );
}

#[test]
fn test_vauchi_error_contact_not_found() {
    let err = VauchiError::ContactNotFound("abc123".to_string());
    let mobile_err: MobileError = err.into();

    let msg = format!("{mobile_err}");
    assert!(
        msg.contains("Contact not found"),
        "Should be ContactNotFound, got: {msg}"
    );
    assert!(
        msg.contains("abc123"),
        "Should preserve contact ID, got: {msg}"
    );
}

#[test]
fn test_vauchi_error_storage_maps_to_storage_error() {
    let storage_err = StorageError::Encryption("key error".to_string());
    let err = VauchiError::Storage(storage_err);
    let mobile_err: MobileError = err.into();

    let msg = format!("{mobile_err}");
    assert!(
        msg.contains("Storage error"),
        "VauchiError::Storage should map to MobileError::StorageError, got: {msg}"
    );
}

#[test]
fn test_vauchi_error_other_maps_to_internal() {
    let err = VauchiError::IdentityNotInitialized;
    let mobile_err: MobileError = err.into();

    let msg = format!("{mobile_err}");
    assert!(
        msg.contains("Internal error"),
        "Unmapped variants should map to Internal, got: {msg}"
    );
}

#[test]
fn test_error_display_not_initialized() {
    let err = MobileError::NotInitialized;
    assert_eq!(
        format!("{err}"),
        "Library not initialized",
        "NotInitialized display message"
    );
}

#[test]
fn test_error_display_already_initialized() {
    let err = MobileError::AlreadyInitialized;
    assert_eq!(
        format!("{err}"),
        "Already initialized",
        "AlreadyInitialized display message"
    );
}

#[test]
fn test_error_display_identity_not_found() {
    let err = MobileError::IdentityNotFound;
    assert_eq!(
        format!("{err}"),
        "Identity not found",
        "IdentityNotFound display message"
    );
}

#[test]
fn test_error_display_invalid_qr() {
    let err = MobileError::InvalidQrCode;
    assert_eq!(
        format!("{err}"),
        "Invalid QR code",
        "InvalidQrCode display message"
    );
}

#[test]
fn test_error_display_exchange_failed() {
    let err = MobileError::ExchangeFailed("timeout".to_string());
    assert_eq!(
        format!("{err}"),
        "Exchange failed: timeout",
        "ExchangeFailed display message"
    );
}

#[test]
fn test_error_display_crypto_error() {
    let err = MobileError::CryptoError("bad key".to_string());
    assert_eq!(
        format!("{err}"),
        "Crypto error: bad key",
        "CryptoError display message"
    );
}

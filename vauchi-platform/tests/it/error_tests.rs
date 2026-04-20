// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for mobile error type conversions and display (ADR-044).
//!
//! After ADR-044 the `MobileError` surface is the eight-variant flat enum.
//! Tests cover (a) Display output per variant, (b) `From` conversions from
//! internal domain errors, and (c) the `Other` escape hatch collapsing
//! formerly distinct variants.

use vauchi_core::{StorageError, VauchiError};
use vauchi_platform::MobileError;

// ── Display format per variant ──────────────────────────────────────

// @internal
#[test]
fn wrong_password_display_is_human_readable() {
    let err = MobileError::WrongPassword;
    assert_eq!(format!("{err}"), "Incorrect password");
}

// @internal
#[test]
fn decrypt_failed_display_is_human_readable() {
    let err = MobileError::DecryptFailed;
    assert_eq!(format!("{err}"), "Decryption failed");
}

// @internal
#[test]
fn invalid_input_display_includes_field_and_message() {
    let err = MobileError::InvalidInput {
        field: "pin".to_string(),
        message: "Must be at least 4 digits".to_string(),
    };
    assert_eq!(
        format!("{err}"),
        "Invalid input on 'pin': Must be at least 4 digits"
    );
}

// @internal
#[test]
fn network_unavailable_display_is_human_readable() {
    let err = MobileError::NetworkUnavailable;
    assert_eq!(format!("{err}"), "Network unavailable");
}

// @internal
#[test]
fn relay_error_display_includes_status_and_message() {
    let err = MobileError::RelayError {
        status: 503,
        message: "service unavailable".to_string(),
    };
    assert_eq!(format!("{err}"), "Relay error 503: service unavailable");
}

// @internal
#[test]
fn rate_limited_display_includes_retry_seconds() {
    let err = MobileError::RateLimited {
        retry_after_secs: 60,
    };
    assert_eq!(format!("{err}"), "Rate limited — retry after 60s");
}

// @internal
#[test]
fn storage_error_display_includes_message() {
    let err = MobileError::StorageError {
        message: "disk full".to_string(),
    };
    assert_eq!(format!("{err}"), "Storage error: disk full");
}

// @internal
#[test]
fn other_display_is_raw_message() {
    let err = MobileError::Other {
        message: "unexpected".to_string(),
    };
    assert_eq!(format!("{err}"), "unexpected");
}

// ── From conversions ────────────────────────────────────────────────

// @internal
#[test]
fn storage_error_from_uses_user_message() {
    let storage_err = StorageError::NotFound("test record".to_string());
    let mobile_err: MobileError = storage_err.into();
    match mobile_err {
        MobileError::StorageError { message } => {
            assert!(
                message.contains("storage error occurred"),
                "expected sanitized user_message (F9 audit), got: {message}"
            );
        }
        other => panic!("expected StorageError, got {other:?}"),
    }
}

// @internal
#[test]
fn vauchi_error_contact_not_found_maps_to_other_with_id() {
    let err = VauchiError::ContactNotFound("abc123".to_string());
    let mobile_err: MobileError = err.into();
    match mobile_err {
        MobileError::Other { message } => {
            assert!(
                message.contains("abc123"),
                "contact id must survive the mapping, got: {message}"
            );
        }
        other => panic!("expected Other, got {other:?}"),
    }
}

// @internal
#[test]
fn vauchi_error_storage_maps_to_storage_error_variant() {
    let storage_err = StorageError::Encryption("key error".to_string());
    let err = VauchiError::Storage(storage_err);
    let mobile_err: MobileError = err.into();
    assert!(
        matches!(mobile_err, MobileError::StorageError { .. }),
        "VauchiError::Storage must map to StorageError variant"
    );
}

// @internal
#[test]
fn vauchi_error_unmapped_variant_collapses_to_other() {
    let err = VauchiError::IdentityNotInitialized;
    let mobile_err: MobileError = err.into();
    assert!(
        matches!(mobile_err, MobileError::Other { .. }),
        "unmapped variants must collapse into Other (ADR-044 escape hatch)"
    );
}

// ── Convenience constructors ────────────────────────────────────────

// @internal
#[test]
fn other_constructor_accepts_str_and_string() {
    let a = MobileError::other("literal");
    let b = MobileError::other(String::from("owned"));
    assert!(matches!(a, MobileError::Other { .. }));
    assert!(matches!(b, MobileError::Other { .. }));
}

// @internal
#[test]
fn invalid_input_constructor_defaults_field_to_empty() {
    let err = MobileError::invalid_input("missing phone");
    match err {
        MobileError::InvalidInput { field, message } => {
            assert_eq!(field, "");
            assert_eq!(message, "missing phone");
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// @internal
#[test]
fn storage_constructor_yields_storage_variant() {
    let err = MobileError::storage("disk full");
    assert!(matches!(err, MobileError::StorageError { .. }));
}

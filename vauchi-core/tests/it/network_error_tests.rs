// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::error
//! Extracted from error.rs

use vauchi_core::network::*;

#[test]
fn test_error_display_messages() {
    let errors = vec![
        (
            NetworkError::ConnectionFailed("refused".into()),
            "Connection failed: refused",
        ),
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

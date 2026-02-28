// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for delivery::error_messages
//! Human-readable failure reason mapping.

use vauchi_core::delivery::failure_to_user_message;

// @scenario: message_delivery:User sees meaningful error for failed delivery
#[test]
fn test_connection_timeout_error_message() {
    let msg = failure_to_user_message("connection_timeout");
    assert!(!msg.is_empty(), "Should produce a message for timeout");
    assert!(
        msg.to_lowercase().contains("relay") || msg.to_lowercase().contains("connection"),
        "Message should mention relay or connection"
    );
}

#[test]
fn test_recipient_not_found_error_message() {
    let msg = failure_to_user_message("recipient_not_found");
    assert!(!msg.is_empty(), "Should produce a message");
    assert!(
        msg.to_lowercase().contains("recipient"),
        "Message should mention recipient"
    );
}

#[test]
fn test_key_mismatch_error_message() {
    let msg = failure_to_user_message("key_mismatch");
    assert!(!msg.is_empty(), "Should produce a message");
    assert!(
        msg.to_lowercase().contains("key") || msg.to_lowercase().contains("verify"),
        "Message should mention key or verification"
    );
}

#[test]
fn test_quota_exceeded_error_message() {
    let msg = failure_to_user_message("quota_exceeded");
    assert!(!msg.is_empty(), "Should produce a message");
    assert!(
        msg.to_lowercase().contains("full") || msg.to_lowercase().contains("storage"),
        "Message should mention full or storage"
    );
}

#[test]
fn test_expired_message_error_message() {
    let msg = failure_to_user_message("expired");
    assert!(!msg.is_empty(), "Should produce a message");
    assert!(
        msg.to_lowercase().contains("expir"),
        "Message should mention expiration"
    );
}

// @scenario: message_delivery:Unknown failure reasons produce generic message
#[test]
fn test_unknown_failure_reason_returns_generic_message() {
    let msg = failure_to_user_message("something_unexpected");
    assert!(
        !msg.is_empty(),
        "Unknown failures should still produce a message"
    );
    assert!(
        msg.to_lowercase().contains("try") || msg.to_lowercase().contains("again"),
        "Generic message should suggest retry"
    );
}

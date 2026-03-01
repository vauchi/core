// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for delivery::key_rotation
//! Key change detection for delivery error handling.

use vauchi_core::delivery::{KeyRotationDetector, KeyRotationError};

// @scenario: message_delivery:Key rotation detected during send
// @scenario: message_delivery.feature:Handle recipient key rotation
#[test]
fn test_detect_key_rotation_on_key_mismatch() {
    let detector = KeyRotationDetector::new();
    let known_key = [0x01u8; 32];
    let different_key = [0x02u8; 32];

    let result = detector.check_key_consistency("alice", &known_key, &different_key);
    assert!(
        matches!(result, Err(KeyRotationError::RecipientKeyChanged { .. })),
        "Should detect key change when keys differ"
    );
}

// @scenario: message_delivery:No rotation when key matches
#[test]
fn test_no_rotation_when_key_matches() {
    let detector = KeyRotationDetector::new();
    let key = [0x01u8; 32];

    let result = detector.check_key_consistency("alice", &key, &key);
    assert!(result.is_ok(), "Should pass when keys match");
}

// @scenario: message_delivery:Key rotation error includes contact ID
#[test]
fn test_key_rotation_error_includes_contact_id() {
    let detector = KeyRotationDetector::new();
    let known_key = [0x01u8; 32];
    let different_key = [0x02u8; 32];

    let result = detector.check_key_consistency("bob", &known_key, &different_key);
    match result {
        Err(KeyRotationError::RecipientKeyChanged { contact_id }) => {
            assert_eq!(contact_id, "bob", "Error should include the contact ID");
        }
        other => panic!("Expected RecipientKeyChanged, got: {:?}", other),
    }
}

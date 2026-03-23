// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for storage::recovery

use vauchi_core::{Storage, SymmetricKey};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// @scenario: contact_recovery :: Accept recovery and reconnect
#[test]
fn test_save_and_get_recovery_response() {
    let storage = test_storage();
    storage
        .save_recovery_response("claim-1", "contact-a", "accept", None)
        .unwrap();

    let result = storage.get_recovery_response("claim-1").unwrap();
    assert!(result.is_some(), "expected Some value");

    let (contact_id, response, remind_at) = result.unwrap();
    assert_eq!(contact_id, "contact-a");
    assert_eq!(response, "accept");
    assert!(remind_at.is_none());
}

#[test]
fn test_recovery_response_with_remind_at() {
    let storage = test_storage();
    storage
        .save_recovery_response("claim-2", "contact-b", "remind_me_later", Some(9999))
        .unwrap();

    let result = storage.get_recovery_response("claim-2").unwrap();
    let (_, response, remind_at) = result.unwrap();
    assert_eq!(response, "remind_me_later");
    assert_eq!(remind_at, Some(9999));
}

#[test]
fn test_recovery_response_overwrite() {
    let storage = test_storage();
    storage
        .save_recovery_response("claim-1", "contact-a", "reject", None)
        .unwrap();
    // Overwrite with accept
    storage
        .save_recovery_response("claim-1", "contact-a", "accept", None)
        .unwrap();

    let (_, response, _) = storage.get_recovery_response("claim-1").unwrap().unwrap();
    assert_eq!(response, "accept");
}

#[test]
fn test_recovery_response_not_found() {
    let storage = test_storage();
    let result = storage.get_recovery_response("nonexistent").unwrap();
    assert!(result.is_none());
}

// @scenario: contact_recovery :: Relay rate limits recovery queries
#[test]
fn test_check_recovery_rate_limit_empty() {
    let storage = test_storage();
    let (count, window_start) = storage.check_recovery_rate_limit(b"some_pk").unwrap();
    assert_eq!(count, 0);
    assert_eq!(window_start, 0);
}

// @scenario: contact_recovery :: Relay rate limits recovery queries
#[test]
fn test_update_and_check_recovery_rate_limit() {
    let storage = test_storage();
    let pk = b"identity_public_key_here_32bytes!";

    storage
        .update_recovery_rate_limit(pk, 3, 1700000000)
        .unwrap();

    let (count, window_start) = storage.check_recovery_rate_limit(pk).unwrap();
    assert_eq!(count, 3);
    assert_eq!(window_start, 1700000000);
}

// @scenario: contact_recovery :: Relay rate limits recovery queries
#[test]
fn test_recovery_rate_limit_overwrite() {
    let storage = test_storage();
    let pk = b"identity_public_key_here_32bytes!";

    storage.update_recovery_rate_limit(pk, 1, 1000).unwrap();
    storage.update_recovery_rate_limit(pk, 5, 2000).unwrap();

    let (count, window_start) = storage.check_recovery_rate_limit(pk).unwrap();
    assert_eq!(count, 5);
    assert_eq!(window_start, 2000);
}

#[test]
fn test_multiple_recovery_responses() {
    let storage = test_storage();
    storage
        .save_recovery_response("claim-1", "contact-a", "accept", None)
        .unwrap();
    storage
        .save_recovery_response("claim-2", "contact-b", "reject", None)
        .unwrap();
    storage
        .save_recovery_response("claim-3", "contact-c", "remind_me_later", Some(5000))
        .unwrap();

    assert!(
        storage.get_recovery_response("claim-1").unwrap().is_some(),
        "expected Some value"
    );
    assert!(
        storage.get_recovery_response("claim-2").unwrap().is_some(),
        "expected Some value"
    );
    assert!(
        storage.get_recovery_response("claim-3").unwrap().is_some(),
        "expected Some value"
    );
}

// ============================================================
// Recovery Settings coverage tests
// ============================================================

use vauchi_core::recovery::RecoverySettings;

/// Test load_recovery_settings returns None when not saved
// @scenario: contact_recovery :: Recovery settings persistence
#[test]
fn test_load_recovery_settings_none() {
    let storage = test_storage();
    let settings = storage.load_recovery_settings().unwrap();
    assert!(settings.is_none(), "No settings saved means None");
}

/// Test save and load recovery settings roundtrip
// @scenario: contact_recovery :: Recovery settings persistence
#[test]
fn test_save_load_recovery_settings() {
    let storage = test_storage();
    let settings = RecoverySettings::new(5, 3).unwrap();

    storage.save_recovery_settings(&settings).unwrap();

    let loaded = storage
        .load_recovery_settings()
        .unwrap()
        .expect("Should load settings");

    assert_eq!(loaded.recovery_threshold(), 5);
    assert_eq!(loaded.verification_threshold(), 3);
}

/// Test recovery settings overwrite
// @scenario: contact_recovery :: Recovery settings persistence
#[test]
fn test_recovery_settings_overwrite() {
    let storage = test_storage();

    let settings1 = RecoverySettings::new(3, 2).unwrap();
    storage.save_recovery_settings(&settings1).unwrap();

    let settings2 = RecoverySettings::new(7, 4).unwrap();
    storage.save_recovery_settings(&settings2).unwrap();

    let loaded = storage.load_recovery_settings().unwrap().unwrap();
    assert_eq!(loaded.recovery_threshold(), 7);
    assert_eq!(loaded.verification_threshold(), 4);
}

/// Test recovery settings default roundtrip
// @scenario: contact_recovery :: Recovery settings persistence
#[test]
fn test_recovery_settings_default_roundtrip() {
    let storage = test_storage();
    let defaults = RecoverySettings::default();

    storage.save_recovery_settings(&defaults).unwrap();

    let loaded = storage.load_recovery_settings().unwrap().unwrap();
    assert_eq!(loaded.recovery_threshold(), defaults.recovery_threshold());
    assert_eq!(
        loaded.verification_threshold(),
        defaults.verification_threshold()
    );
}

/// Test rate limits are independent per identity PK
// @scenario: contact_recovery :: Relay rate limits recovery queries
#[test]
fn test_recovery_rate_limit_independent_per_pk() {
    let storage = test_storage();

    let pk_a = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let pk_b = b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    storage.update_recovery_rate_limit(pk_a, 2, 1000).unwrap();
    storage.update_recovery_rate_limit(pk_b, 5, 2000).unwrap();

    let (count_a, window_a) = storage.check_recovery_rate_limit(pk_a).unwrap();
    assert_eq!(count_a, 2);
    assert_eq!(window_a, 1000);

    let (count_b, window_b) = storage.check_recovery_rate_limit(pk_b).unwrap();
    assert_eq!(count_b, 5);
    assert_eq!(window_b, 2000);
}

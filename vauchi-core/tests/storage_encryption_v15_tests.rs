// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for Phase 2c: Low-priority table encryption (v15).
//!
//! Verifies that field_validations, ux_state, and audit_log
//! store data encrypted and roundtrip correctly.
//!
//! Tables intentionally skipped:
//! - `replay_nonces`: contains only random nonces + timestamps, no personal data
//! - `consent_records`: consent decisions (type, granted) aren't personal data;
//!   columns are needed for functional queries (`check_consent`)

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::social::ProfileValidation;
use vauchi_core::storage::Storage;
use vauchi_core::types::{AhaMomentTracker, AhaMomentType, DemoContactState};

fn open_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

fn make_contact(name: &str) -> Contact {
    Contact::from_exchange([1u8; 32], ContactCard::new(name), SymmetricKey::generate())
}

// === Migration Tests ===

#[test]
fn test_migration_v15_adds_encrypted_columns() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    // field_validations: field_value_encrypted, signature_encrypted
    let has_fv_encrypted: bool = raw_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('field_validations') WHERE name = 'field_value_encrypted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        has_fv_encrypted,
        "field_validations should have field_value_encrypted column"
    );

    let has_sig_encrypted: bool = raw_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('field_validations') WHERE name = 'signature_encrypted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        has_sig_encrypted,
        "field_validations should have signature_encrypted column"
    );

    // ux_state: aha_tracker_json_encrypted, demo_contact_json_encrypted
    let has_aha_encrypted: bool = raw_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('ux_state') WHERE name = 'aha_tracker_json_encrypted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        has_aha_encrypted,
        "ux_state should have aha_tracker_json_encrypted column"
    );

    let has_demo_encrypted: bool = raw_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('ux_state') WHERE name = 'demo_contact_json_encrypted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        has_demo_encrypted,
        "ux_state should have demo_contact_json_encrypted column"
    );

    // audit_log: details_encrypted
    let has_details_encrypted: bool = raw_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('audit_log') WHERE name = 'details_encrypted'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        has_details_encrypted,
        "audit_log should have details_encrypted column"
    );
}

#[test]
fn test_schema_version_at_least_16() {
    let (_dir, storage) = open_storage();
    let version = storage.schema_version().unwrap();
    assert!(
        version >= 16,
        "schema version should be at least 16, got {}",
        version
    );
}

// === field_validations roundtrip tests ===

#[test]
fn test_field_validation_roundtrip() {
    let (_dir, storage) = open_storage();

    let contact = make_contact("Test User");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id();

    let field_id = format!("{}:email", contact_id);
    let signature = [42u8; 64];
    let validation = ProfileValidation::from_stored(
        &field_id,
        "test@example.com",
        "validator-123",
        1700000000,
        signature,
    );

    storage.save_validation(&validation).unwrap();

    let loaded = storage
        .load_validations_for_field(contact_id, "email")
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].field_value(), "test@example.com");
    assert_eq!(loaded[0].validator_id(), "validator-123");
    assert_eq!(loaded[0].validated_at(), 1700000000);
    assert_eq!(*loaded[0].signature(), signature);
}

#[test]
fn test_field_validation_encrypted_in_db() {
    let (dir, storage) = open_storage();

    let contact = make_contact("Test User");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id();

    let field_id = format!("{}:phone", contact_id);
    let validation = ProfileValidation::from_stored(
        &field_id,
        "+1234567890",
        "validator-456",
        1700000000,
        [99u8; 64],
    );

    storage.save_validation(&validation).unwrap();
    drop(storage);

    // Check raw DB: field_value should be cleared, encrypted column should have data
    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    type ValidationRow = (String, Option<Vec<u8>>, Vec<u8>, Option<Vec<u8>>);
    let (fv_plain, fv_enc, sig_plain, sig_enc): ValidationRow = raw_conn
        .query_row(
            "SELECT field_value, field_value_encrypted, signature, signature_encrypted FROM field_validations LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert!(
        fv_plain.is_empty(),
        "plaintext field_value should be cleared"
    );
    assert!(
        fv_enc.is_some() && !fv_enc.unwrap().is_empty(),
        "field_value_encrypted should have data"
    );
    assert!(
        sig_plain.is_empty(),
        "plaintext signature should be cleared"
    );
    assert!(
        sig_enc.is_some() && !sig_enc.unwrap().is_empty(),
        "signature_encrypted should have data"
    );
}

#[test]
fn test_field_validation_by_validator_roundtrip() {
    let (_dir, storage) = open_storage();

    let contact = make_contact("Test User");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id();

    let field_id = format!("{}:email", contact_id);
    let validation = ProfileValidation::from_stored(
        &field_id,
        "user@example.com",
        "my-validator-id",
        1700000000,
        [11u8; 64],
    );

    storage.save_validation(&validation).unwrap();

    let loaded = storage
        .load_validations_by_validator("my-validator-id")
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].field_value(), "user@example.com");
}

#[test]
fn test_field_validation_has_validated_works() {
    let (_dir, storage) = open_storage();

    let contact = make_contact("Test User");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id();

    let field_id = format!("{}:email", contact_id);
    let validation = ProfileValidation::from_stored(
        &field_id,
        "user@example.com",
        "validator-xyz",
        1700000000,
        [22u8; 64],
    );

    storage.save_validation(&validation).unwrap();

    assert!(
        storage
            .has_validated(contact_id, "email", "validator-xyz")
            .unwrap()
    );
    assert!(
        !storage
            .has_validated(contact_id, "email", "other-validator")
            .unwrap()
    );
}

// === ux_state roundtrip tests ===

#[test]
fn test_aha_tracker_roundtrip_encrypted() {
    let (_dir, storage) = open_storage();

    let mut tracker = AhaMomentTracker::new();
    tracker.mark_seen(AhaMomentType::CardCreationComplete);
    tracker.mark_seen(AhaMomentType::FirstEdit);

    storage.save_aha_tracker(&tracker).unwrap();
    let loaded = storage.load_aha_tracker().unwrap().unwrap();

    assert!(loaded.has_seen(AhaMomentType::CardCreationComplete));
    assert!(loaded.has_seen(AhaMomentType::FirstEdit));
    assert!(!loaded.has_seen(AhaMomentType::FirstContactAdded));
}

#[test]
fn test_demo_contact_state_roundtrip_encrypted() {
    let (_dir, storage) = open_storage();

    let mut state = DemoContactState::new_active();
    state.advance_to_next_tip();
    state.advance_to_next_tip();

    storage.save_demo_contact_state(&state).unwrap();
    let loaded = storage.load_demo_contact_state().unwrap().unwrap();

    assert!(loaded.is_active);
    assert_eq!(loaded.update_count, 2);
}

#[test]
fn test_ux_state_combined_roundtrip_encrypted() {
    let (_dir, storage) = open_storage();

    let mut tracker = AhaMomentTracker::new();
    tracker.mark_seen(AhaMomentType::CardCreationComplete);

    let mut demo_state = DemoContactState::new_active();
    demo_state.advance_to_next_tip();

    storage.save_ux_state(&tracker, &demo_state).unwrap();

    let (loaded_tracker, loaded_demo) = storage.load_ux_state().unwrap();
    assert!(loaded_tracker.has_seen(AhaMomentType::CardCreationComplete));
    assert!(loaded_demo.is_active);
    assert_eq!(loaded_demo.update_count, 1);
}

#[test]
fn test_ux_state_encrypted_in_db() {
    let (dir, storage) = open_storage();

    let mut tracker = AhaMomentTracker::new();
    tracker.mark_seen(AhaMomentType::FirstEdit);

    let demo_state = DemoContactState::new_active();
    storage.save_ux_state(&tracker, &demo_state).unwrap();
    drop(storage);

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    type UxStateRow = (
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
        Option<Vec<u8>>,
    );
    let (aha_plain, aha_enc, demo_plain, demo_enc): UxStateRow = raw_conn
        .query_row(
            "SELECT aha_tracker_json, aha_tracker_json_encrypted, demo_contact_json, demo_contact_json_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    // Plaintext columns should be cleared
    assert!(
        aha_plain.is_none_or(|s| s.is_empty()),
        "plaintext aha_tracker_json should be cleared"
    );
    assert!(
        demo_plain.is_none_or(|s| s.is_empty()),
        "plaintext demo_contact_json should be cleared"
    );
    // Encrypted columns should have data
    assert!(
        aha_enc.is_some() && !aha_enc.unwrap().is_empty(),
        "aha_tracker_json_encrypted should have data"
    );
    assert!(
        demo_enc.is_some() && !demo_enc.unwrap().is_empty(),
        "demo_contact_json_encrypted should have data"
    );
}

// === audit_log roundtrip tests ===

#[test]
fn test_audit_log_roundtrip() {
    // allow(zero_assertions): Write-only interface — no read-back API to assert against
    let (_dir, storage) = open_storage();

    storage
        .log_audit_event("test_event", Some("detailed info here"))
        .unwrap();
    storage.log_audit_event("another_event", None).unwrap();
}

#[test]
fn test_audit_log_encrypted_in_db() {
    let (dir, storage) = open_storage();

    storage
        .log_audit_event("data_deleted", Some("Deleted contact John"))
        .unwrap();
    drop(storage);

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let (details_plain, details_enc): (Option<String>, Option<Vec<u8>>) = raw_conn
        .query_row(
            "SELECT details, details_encrypted FROM audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    // Plaintext details should be cleared
    assert!(
        details_plain.is_none_or(|s| s.is_empty()),
        "plaintext details should be cleared"
    );
    // Encrypted details should have data
    assert!(
        details_enc.is_some() && !details_enc.unwrap().is_empty(),
        "details_encrypted should have data"
    );
}

#[test]
fn test_audit_log_null_details_no_encryption() {
    let (dir, storage) = open_storage();

    storage.log_audit_event("simple_event", None).unwrap();
    drop(storage);

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let (details_plain, details_enc): (Option<String>, Option<Vec<u8>>) = raw_conn
        .query_row(
            "SELECT details, details_encrypted FROM audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    // When details is None, both should be empty/null
    assert!(details_plain.is_none() || details_plain.unwrap().is_empty());
    assert!(details_enc.is_none() || details_enc.unwrap().is_empty());
}

// === Rekey tests ===

#[test]
fn test_rekey_preserves_field_validations() {
    let (_dir, mut storage) = open_storage();

    let contact = make_contact("Rekey Test");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();

    let field_id = format!("{}:email", contact_id);
    let validation = ProfileValidation::from_stored(
        &field_id,
        "rekey@example.com",
        "validator-rekey",
        1700000000,
        [77u8; 64],
    );
    storage.save_validation(&validation).unwrap();

    // Rekey
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .load_validations_for_field(&contact_id, "email")
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].field_value(), "rekey@example.com");
    assert_eq!(*loaded[0].signature(), [77u8; 64]);
}

#[test]
fn test_rekey_preserves_ux_state() {
    let (_dir, mut storage) = open_storage();

    let mut tracker = AhaMomentTracker::new();
    tracker.mark_seen(AhaMomentType::FirstContactAdded);
    let demo_state = DemoContactState::new_active();
    storage.save_ux_state(&tracker, &demo_state).unwrap();

    // Rekey
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let (loaded_tracker, loaded_demo) = storage.load_ux_state().unwrap();
    assert!(loaded_tracker.has_seen(AhaMomentType::FirstContactAdded));
    assert!(loaded_demo.is_active);
}

#[test]
fn test_rekey_preserves_audit_log() {
    let (dir, mut storage) = open_storage();

    storage
        .log_audit_event("test_event", Some("sensitive details"))
        .unwrap();

    // Rekey
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();
    drop(storage);

    // Verify at DB level that the record still exists
    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = raw_conn
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

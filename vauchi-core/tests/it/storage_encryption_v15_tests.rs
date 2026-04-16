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

// === Rekey: personal notes and field notes self-healing ===

// @scenario: security :: Rekey heals plaintext personal notes
#[test]
fn test_rekey_heals_plaintext_personal_notes() {
    let (dir, mut storage) = open_storage();
    let contact = make_contact("Notes Rekey");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();

    // Write plaintext directly to the DB column, simulating the legacy gap
    // where callers wrote raw UTF-8 to personal_notes_encrypted.
    let db_path = dir.path().join("vauchi.db");
    {
        let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
        raw_conn
            .execute(
                "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
                rusqlite::params!["Met at conference".as_bytes(), &contact_id],
            )
            .unwrap();
    }

    // Rekey should NOT crash — rekey_or_heal detects plaintext, encrypts it.
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should be readable after rekey (load decrypts transparently).
    let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();
    assert_eq!(
        String::from_utf8(loaded).unwrap(),
        "Met at conference",
        "Plaintext notes should survive rekey via self-healing"
    );

    // Verify the DB column now contains encrypted data (starts with algorithm tag).
    {
        let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
        let raw: Vec<u8> = raw_conn
            .query_row(
                "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
                rusqlite::params![&contact_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            raw[0] == 0x02 || raw[0] == 0x03,
            "After rekey, DB should contain encrypted data (tag 0x02 or 0x03), got 0x{:02x}",
            raw[0]
        );
    }
}

// @scenario: security :: Rekey preserves encrypted personal notes
#[test]
fn test_rekey_preserves_encrypted_personal_notes() {
    let (_dir, mut storage) = open_storage();
    let contact = make_contact("Enc Notes Rekey");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();

    // Write notes through the API (encrypts at storage layer).
    let note_text = "Properly encrypted note";
    storage
        .save_personal_notes(&contact_id, note_text.as_bytes())
        .unwrap();

    // Rekey
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should still be readable.
    let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();
    assert_eq!(
        String::from_utf8(loaded).unwrap(),
        note_text,
        "Encrypted notes should survive rekey via normal decrypt+re-encrypt"
    );
}

// @scenario: security :: Rekey heals plaintext contact field notes
#[test]
fn test_rekey_heals_plaintext_field_notes() {
    let (dir, mut storage) = open_storage();
    let contact = make_contact("Field Notes Rekey");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();
    let field_id = "email-1";

    // Write plaintext directly to the DB, simulating the legacy gap.
    let db_path = dir.path().join("vauchi.db");
    {
        let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
        raw_conn
            .execute(
                "INSERT INTO contact_field_notes (contact_id, field_id, note_encrypted, updated_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![&contact_id, field_id, "work email".as_bytes(), 1000i64],
            )
            .unwrap();
    }

    // Rekey should NOT crash.
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should be readable after rekey.
    let loaded = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(loaded.len(), 1, "Should have one field note");
    assert_eq!(
        String::from_utf8(loaded[field_id].clone()).unwrap(),
        "work email",
        "Plaintext field notes should survive rekey via self-healing"
    );

    // Verify the DB column now contains encrypted data.
    {
        let raw_conn = rusqlite::Connection::open(&db_path).unwrap();
        let raw: Vec<u8> = raw_conn
            .query_row(
                "SELECT note_encrypted FROM contact_field_notes WHERE contact_id = ?1 AND field_id = ?2",
                rusqlite::params![&contact_id, field_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            raw[0] == 0x02 || raw[0] == 0x03,
            "After rekey, field note should be encrypted (tag 0x02 or 0x03), got 0x{:02x}",
            raw[0]
        );
    }
}

// @scenario: security :: Rekey preserves encrypted contact field notes
#[test]
fn test_rekey_preserves_encrypted_field_notes() {
    let (_dir, mut storage) = open_storage();
    let contact = make_contact("Enc Field Rekey");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();
    let field_id = "phone-1";

    // Write notes through the API (encrypts at storage layer).
    storage
        .save_contact_field_note(&contact_id, field_id, b"personal phone")
        .unwrap();

    // Rekey
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should still be readable.
    let loaded = storage.load_contact_field_notes(&contact_id).unwrap();
    assert_eq!(
        String::from_utf8(loaded[field_id].clone()).unwrap(),
        "personal phone",
        "Encrypted field notes should survive rekey via normal decrypt+re-encrypt"
    );
}

// @scenario: security :: Notes stored at rest are encrypted
#[test]
fn test_notes_stored_encrypted_at_rest() {
    let (dir, storage) = open_storage();
    let contact = make_contact("AtRest Test");
    storage.save_contact(&contact).unwrap();
    let contact_id = contact.id().to_string();

    // Save via API
    storage
        .save_personal_notes(&contact_id, b"secret note")
        .unwrap();
    storage
        .save_contact_field_note(&contact_id, "f1", b"field secret")
        .unwrap();
    drop(storage);

    // Open raw DB — plaintext should NOT be visible.
    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let raw_notes: Vec<u8> = raw_conn
        .query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            rusqlite::params![&contact_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(
        raw_notes, b"secret note",
        "Personal notes must be encrypted at rest, not plaintext"
    );

    let raw_field: Vec<u8> = raw_conn
        .query_row(
            "SELECT note_encrypted FROM contact_field_notes WHERE contact_id = ?1",
            rusqlite::params![&contact_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(
        raw_field, b"field secret",
        "Field notes must be encrypted at rest, not plaintext"
    );
}

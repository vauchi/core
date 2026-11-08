// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::storage::Storage;
use vauchi_core::{Contact, ContactCard, ImportSource, SymmetricKey};

fn contact(id: &str) -> Contact {
    Contact::from_import(
        id.into(),
        ContactCard::new("Preserved card"),
        ImportSource::VcardFile,
        None,
        0,
    )
}

// @scenario: security :: Local database encryption
#[test]
fn assumption_rekey_query_failures_preserve_the_active_key_and_readable_data() {
    for table in [
        "identity",
        "own_card",
        "device_registry",
        "device_info",
        "version_vector",
        "sync_field_timestamps",
        "deletion_state",
        "emergency_config",
        "recovery_settings",
        "recovery_progress",
    ] {
        let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
        storage
            .contacts()
            .save_contact(&contact("preserved"))
            .unwrap();
        let old_key = *storage.key().as_bytes();
        storage
            .connection()
            .execute_batch(&format!("ALTER TABLE {table} RENAME TO unavailable_table"))
            .unwrap();
        let error = storage.rekey(SymmetricKey::generate()).expect_err(table);
        assert!(error.to_string().contains(table), "{table}: {error}");
        assert_eq!(*storage.key().as_bytes(), old_key, "{table}");
        assert_eq!(
            storage
                .contacts()
                .load_contact("preserved")
                .unwrap()
                .unwrap()
                .display_name(),
            "Preserved card",
            "{table}"
        );
        storage
            .connection()
            .execute_batch(&format!("ALTER TABLE unavailable_table RENAME TO {table}"))
            .unwrap();
        let new_key = SymmetricKey::generate();
        let expected_key = *new_key.as_bytes();
        storage.rekey(new_key).unwrap();
        assert_eq!(*storage.key().as_bytes(), expected_key);
        assert_eq!(
            storage
                .contacts()
                .load_contact("preserved")
                .unwrap()
                .unwrap()
                .display_name(),
            "Preserved card"
        );
    }
}

// @scenario: security :: Local database encryption
#[test]
fn assumption_rekey_row_conversion_failure_preserves_the_key_and_contact() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .contacts()
        .save_contact(&contact("preserved"))
        .unwrap();
    let old_key = *storage.key().as_bytes();
    storage.connection().execute_batch("INSERT INTO ux_state (id, updated_at, aha_tracker_json_encrypted) VALUES (1, 0, 'not a blob') ON CONFLICT(id) DO UPDATE SET aha_tracker_json_encrypted = 'not a blob'").unwrap();
    let error = storage
        .rekey(SymmetricKey::generate())
        .expect_err("invalid SQL value must not become absent optional data");
    assert!(error.to_string().contains("Invalid column type"), "{error}");
    assert_eq!(*storage.key().as_bytes(), old_key);
    assert_eq!(
        storage
            .contacts()
            .load_contact("preserved")
            .unwrap()
            .unwrap()
            .display_name(),
        "Preserved card"
    );
}

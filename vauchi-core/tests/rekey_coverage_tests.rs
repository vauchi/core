// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Self-healing rekey coverage tests.
//!
//! These tests ensure that every encrypted column in the schema is handled by
//! the rekey operation. The PRAGMA-based discovery test catches drift when a new
//! migration adds an encrypted column without updating the rekey registry.

use std::collections::BTreeSet;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{ENCRYPTED_COLUMNS, REKEY_SKIP_COLUMNS, Storage};

fn open_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

// === Self-Healing Exhaustiveness Test ===

/// Discovers all encrypted columns from the live schema via PRAGMA table_info
/// and asserts they match the ENCRYPTED_COLUMNS registry in rekey.rs.
///
/// If a migration adds a new `_encrypted`/`_hmac`/`encrypted_blob` column
/// without updating the registry, this test fails with a clear message.
// @scenario: security :: Rekey covers all encrypted columns
#[test]
fn test_rekey_covers_all_encrypted_columns() {
    let (_dir, storage) = open_storage();
    let conn = storage.connection();

    // Step 1: Discover all tables in the schema
    let mut tables: Vec<String> = Vec::new();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != 'schema_version'")
        .unwrap();
    let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
    for row in rows {
        tables.push(row.unwrap());
    }

    // Step 2: For each table, discover columns ending in _encrypted, _hmac, or named encrypted_blob
    let mut schema_columns: BTreeSet<(String, String)> = BTreeSet::new();
    for table in &tables {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .unwrap();
        let cols = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        for col in cols {
            let col_name = col.unwrap();
            if col_name.ends_with("_encrypted")
                || col_name.ends_with("_hmac")
                || col_name == "encrypted_blob"
            {
                schema_columns.insert((table.clone(), col_name));
            }
        }
    }

    // Step 3: Build the registry set
    let registry_columns: BTreeSet<(String, String)> = ENCRYPTED_COLUMNS
        .iter()
        .map(|(t, c)| (t.to_string(), c.to_string()))
        .collect();

    // Step 4: Build the skip set
    let skip_columns: BTreeSet<(String, String)> = REKEY_SKIP_COLUMNS
        .iter()
        .map(|(t, c, _reason)| (t.to_string(), c.to_string()))
        .collect();

    // Step 5: Compare — every schema column must be in registry OR skip list
    let mut missing: Vec<String> = Vec::new();
    for (table, col) in &schema_columns {
        if !registry_columns.contains(&(table.clone(), col.clone()))
            && !skip_columns.contains(&(table.clone(), col.clone()))
        {
            missing.push(format!("{}.{}", table, col));
        }
    }

    assert!(
        missing.is_empty(),
        "Encrypted columns not handled by rekey (add to ENCRYPTED_COLUMNS or REKEY_SKIP_COLUMNS in rekey.rs):\n  {}",
        missing.join("\n  ")
    );

    // Step 6: Reverse check — every registry column must exist in schema
    let mut extra: Vec<String> = Vec::new();
    for (table, col) in &registry_columns {
        if !schema_columns.contains(&(table.clone(), col.clone())) {
            extra.push(format!("{}.{}", table, col));
        }
    }

    assert!(
        extra.is_empty(),
        "ENCRYPTED_COLUMNS entries not found in schema (stale entries?):\n  {}",
        extra.join("\n  ")
    );
}

// === Rekey Roundtrip Tests for Previously-Missing Columns ===

// @scenario: security :: Rekey preserves contacts.cek_encrypted
#[test]
fn test_rekey_preserves_cek_encrypted() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    // Write a test CEK
    let test_cek =
        vauchi_core::crypto::encrypt(&key1, b"test-cek-bytes-32bytes-padding!!").unwrap();
    let card_enc = vauchi_core::crypto::encrypt(&key1, b"{\"name\":\"Test\"}").unwrap();
    let sk_enc = vauchi_core::crypto::encrypt(&key1, b"shared-key-placeholder-32bytes!").unwrap();
    storage.connection().execute(
        "INSERT INTO contacts (id, public_key, display_name, card_encrypted, shared_key_encrypted, cek_encrypted, exchange_timestamp, contact_kind) VALUES ('c1', X'0101010101010101010101010101010101010101010101010101010101010101', '', ?1, ?2, ?3, 1000, 'exchanged')",
        rusqlite::params![card_enc, sk_enc, test_cek],
    ).unwrap();

    // Rekey
    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    // Verify readable with new key
    let loaded: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT cek_encrypted FROM contacts WHERE id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let plain = vauchi_core::crypto::decrypt(&key2, &loaded).unwrap();
    assert_eq!(plain, b"test-cek-bytes-32bytes-padding!!");
}

// @scenario: security :: Rekey preserves identity.password_hash_encrypted
#[test]
fn test_rekey_preserves_password_hash() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let test_hash = vauchi_core::crypto::encrypt(&key1, b"argon2id-hash-placeholder").unwrap();
    let backup_enc = vauchi_core::crypto::encrypt(&key1, b"backup-data").unwrap();
    storage.connection().execute(
        "INSERT OR REPLACE INTO identity (id, backup_data_encrypted, display_name, password_hash_encrypted, created_at) VALUES (1, ?1, 'test', ?2, 1000)",
        rusqlite::params![backup_enc, test_hash],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT password_hash_encrypted FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let plain = vauchi_core::crypto::decrypt(&key2, &loaded).unwrap();
    assert_eq!(plain, b"argon2id-hash-placeholder");
}

// @scenario: security :: Rekey preserves duress_settings
#[test]
fn test_rekey_preserves_duress_settings() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let ids_enc = vauchi_core::crypto::encrypt(&key1, b"[\"contact-a\"]").unwrap();
    let msg_enc = vauchi_core::crypto::encrypt(&key1, b"help me").unwrap();
    storage.connection().execute(
        "INSERT INTO duress_settings (id, alert_contact_ids_encrypted, alert_message_encrypted, created_at, updated_at) VALUES (1, ?1, ?2, 1000, 1000)",
        rusqlite::params![ids_enc, msg_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let (ids_loaded, msg_loaded): (Vec<u8>, Vec<u8>) = storage.connection().query_row(
        "SELECT alert_contact_ids_encrypted, alert_message_encrypted FROM duress_settings WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &ids_loaded).unwrap(),
        b"[\"contact-a\"]"
    );
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &msg_loaded).unwrap(),
        b"help me"
    );
}

// @scenario: security :: Rekey preserves decoy_contacts
#[test]
fn test_rekey_preserves_decoy_contacts() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let card_enc = vauchi_core::crypto::encrypt(&key1, b"{\"name\":\"Decoy\"}").unwrap();
    storage.connection().execute(
        "INSERT INTO decoy_contacts (id, display_name, card_encrypted, created_at, updated_at) VALUES ('d1', 'Decoy', ?1, 1000, 1000)",
        [&card_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT card_encrypted FROM decoy_contacts WHERE id = 'd1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &loaded).unwrap(),
        b"{\"name\":\"Decoy\"}"
    );
}

// @scenario: security :: Rekey preserves emergency_config
#[test]
fn test_rekey_preserves_emergency_config() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let ids_enc = vauchi_core::crypto::encrypt(&key1, b"[\"trusted-1\"]").unwrap();
    let msg_enc = vauchi_core::crypto::encrypt(&key1, b"emergency msg").unwrap();
    storage.connection().execute(
        "INSERT INTO emergency_config (id, trusted_contact_ids_encrypted, message_encrypted, created_at, updated_at) VALUES (1, ?1, ?2, 1000, 1000)",
        rusqlite::params![ids_enc, msg_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let (ids_loaded, msg_loaded): (Vec<u8>, Vec<u8>) = storage.connection().query_row(
        "SELECT trusted_contact_ids_encrypted, message_encrypted FROM emergency_config WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &ids_loaded).unwrap(),
        b"[\"trusted-1\"]"
    );
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &msg_loaded).unwrap(),
        b"emergency msg"
    );
}

// @scenario: security :: Rekey preserves recovery_settings
#[test]
fn test_rekey_preserves_recovery_settings() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let settings_enc = vauchi_core::crypto::encrypt(&key1, b"{\"method\":\"pin\"}").unwrap();
    storage
        .connection()
        .execute(
            "INSERT INTO recovery_settings (id, settings_encrypted, updated_at) VALUES (1, ?1, 1000)",
            [&settings_enc],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT settings_encrypted FROM recovery_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &loaded).unwrap(),
        b"{\"method\":\"pin\"}"
    );
}

// @scenario: security :: Rekey preserves exchange_states
#[test]
fn test_rekey_preserves_exchange_states() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let blob_enc = vauchi_core::crypto::encrypt(&key1, b"{\"state\":\"link-pending\"}").unwrap();
    storage.connection().execute(
        "INSERT INTO exchange_states (exchange_id, encrypted_blob, created_at, expires_at) VALUES ('ex1', ?1, 1000, 2000)",
        [&blob_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT encrypted_blob FROM exchange_states WHERE exchange_id = 'ex1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        vauchi_core::crypto::decrypt(&key2, &loaded).unwrap(),
        b"{\"state\":\"link-pending\"}"
    );
}

// @scenario: security :: Rekey preserves ux_state.onboarding_progress_encrypted
#[test]
fn test_rekey_preserves_onboarding_progress() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let progress_enc = vauchi_core::crypto::encrypt(&key1, b"{\"step\":\"backup\"}").unwrap();
    // ux_state may already have a row from migrations
    storage.connection().execute(
        "INSERT OR REPLACE INTO ux_state (id, aha_tracker_json, demo_contact_json, onboarding_progress_encrypted, updated_at) VALUES (1, '', '', ?1, 1000)",
        [&progress_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Option<Vec<u8>> = storage
        .connection()
        .query_row(
            "SELECT onboarding_progress_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let plain = vauchi_core::crypto::decrypt(&key2, &loaded.unwrap()).unwrap();
    assert_eq!(plain, b"{\"step\":\"backup\"}");
}

// @scenario: security :: Rekey preserves visibility_labels.display_name_override_encrypted
#[test]
fn test_rekey_preserves_label_display_name_override() {
    let (_dir, mut storage) = open_storage();
    let key1 = storage.key().clone();

    let contacts_enc = vauchi_core::crypto::encrypt(&key1, b"[]").unwrap();
    let override_enc = vauchi_core::crypto::encrypt(&key1, b"Custom Label").unwrap();
    storage.connection().execute(
        "INSERT INTO visibility_labels (id, name, contacts_json, visible_fields_json, contacts_json_encrypted, display_name_override_encrypted, created_at, modified_at) VALUES ('l1', 'test', '[]', '[]', ?1, ?2, 1000, 1000)",
        rusqlite::params![contacts_enc, override_enc],
    ).unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let loaded: Option<Vec<u8>> = storage
        .connection()
        .query_row(
            "SELECT display_name_override_encrypted FROM visibility_labels WHERE id = 'l1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let plain = vauchi_core::crypto::decrypt(&key2, &loaded.unwrap()).unwrap();
    assert_eq!(plain, b"Custom Label");
}

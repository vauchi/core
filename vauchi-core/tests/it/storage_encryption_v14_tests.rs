// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for Phase 2b: Medium-priority table encryption (v14).
//!
//! Verifies that device_info, version_vector, contact_sync_timestamps,
//! pending_updates, retry_entries, device_sync_checkpoints,
//! recovery_responses, deletion_state, and sync_checkpoints
//! store data encrypted and roundtrip correctly.

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{DeletionState, PendingUpdate, RetryEntry, Storage, UpdateStatus};
use vauchi_core::sync::VersionVector;

fn open_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

// === Migration Tests ===

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_migration_v14_adds_encrypted_columns() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    // device_info
    raw_conn
        .prepare("SELECT device_info_encrypted FROM device_info LIMIT 0")
        .expect("device_info.device_info_encrypted column should exist");

    // version_vector
    raw_conn
        .prepare("SELECT vector_json_encrypted FROM version_vector LIMIT 0")
        .expect("version_vector.vector_json_encrypted column should exist");

    // contact_sync_timestamps
    raw_conn
        .prepare("SELECT last_sync_at_encrypted FROM contact_sync_timestamps LIMIT 0")
        .expect("contact_sync_timestamps.last_sync_at_encrypted column should exist");

    // pending_updates
    raw_conn
        .prepare("SELECT payload_encrypted FROM pending_updates LIMIT 0")
        .expect("pending_updates.payload_encrypted column should exist");

    // retry_entries
    raw_conn
        .prepare("SELECT payload_encrypted FROM retry_entries LIMIT 0")
        .expect("retry_entries.payload_encrypted column should exist");

    // device_sync_checkpoints
    raw_conn
        .prepare("SELECT items_json_encrypted FROM device_sync_checkpoints LIMIT 0")
        .expect("device_sync_checkpoints.items_json_encrypted column should exist");

    // recovery_responses
    raw_conn
        .prepare("SELECT response_encrypted FROM recovery_responses LIMIT 0")
        .expect("recovery_responses.response_encrypted column should exist");

    // deletion_state
    raw_conn
        .prepare("SELECT state_json_encrypted FROM deletion_state LIMIT 0")
        .expect("deletion_state.state_json_encrypted column should exist");

    // sync_checkpoints
    raw_conn
        .prepare("SELECT state_json_encrypted FROM sync_checkpoints LIMIT 0")
        .expect("sync_checkpoints.state_json_encrypted column should exist");
}

// @internal
#[test]
fn test_migration_v14_schema_version() {
    let (dir, _storage) = open_storage();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let version: u32 = raw_conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        version >= 15,
        "Schema version should be at least 15, got {}",
        version
    );
}

// === Device Info Encryption ===

// @internal
#[test]
fn test_device_info_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let device_id = [0x42; 32];
    storage
        .device()
        .save_device_info(&device_id, 1, "Test Phone", 1000)
        .unwrap();

    let loaded = storage
        .device()
        .load_device_info()
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.0, device_id);
    assert_eq!(loaded.1, 1);
    assert_eq!(loaded.2, "Test Phone");
    assert_eq!(loaded.3, 1000);
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_device_info_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    storage
        .device()
        .save_device_info(&[0x42; 32], 1, "SecretDevice", 1000)
        .unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT device_info_encrypted FROM device_info WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    let name: String = raw_conn
        .query_row(
            "SELECT device_name FROM device_info WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "", "Plaintext device_name should be cleared");
}

// === Version Vector Encryption ===

// @internal
#[test]
fn test_version_vector_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let mut vector = VersionVector::new();
    vector.increment(&[0x01; 32]);
    vector.increment(&[0x02; 32]);

    storage.sync().save_version_vector(&vector).unwrap();

    let loaded = storage
        .sync()
        .load_version_vector()
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.get(&[0x01; 32]), vector.get(&[0x01; 32]));
    assert_eq!(loaded.get(&[0x02; 32]), vector.get(&[0x02; 32]));
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_version_vector_stored_as_encrypted_blob() {
    let (dir, storage) = open_storage();

    let mut vector = VersionVector::new();
    vector.increment(&[0x01; 32]);
    storage.sync().save_version_vector(&vector).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT vector_json_encrypted FROM version_vector WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    // Plaintext should be cleared
    let json: String = raw_conn
        .query_row(
            "SELECT vector_json FROM version_vector WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(json, "", "Plaintext vector_json should be cleared");
}

// === Contact Sync Timestamps Encryption ===

// @internal
#[test]
fn test_contact_sync_timestamps_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    storage
        .sync()
        .set_contact_last_sync("contact-1", 1234567890)
        .unwrap();

    let loaded = storage
        .sync()
        .get_contact_last_sync("contact-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded, 1234567890);
}

// @internal
#[test]
fn test_contact_sync_timestamps_stored_encrypted() {
    let (dir, storage) = open_storage();

    storage
        .sync()
        .set_contact_last_sync("contact-1", 1234567890)
        .unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT last_sync_at_encrypted FROM contact_sync_timestamps WHERE contact_id = ?1",
            ["contact-1"],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());
}

// === Pending Updates Encryption ===

// @internal
#[test]
fn test_pending_update_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let update = PendingUpdate {
        id: "upd-1".to_string(),
        contact_id: "contact-1".to_string(),
        update_type: "card_update".to_string(),
        payload: b"sensitive payload data".to_vec(),
        created_at: 1000,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };

    storage.pending().queue_update(&update).unwrap();

    let loaded = storage
        .pending()
        .get_pending_update("upd-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.payload, b"sensitive payload data");
    assert_eq!(loaded.update_type, "card_update");
    assert_eq!(loaded.contact_id, "contact-1");
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_pending_update_payload_stored_encrypted() {
    let (dir, storage) = open_storage();

    let update = PendingUpdate {
        id: "upd-1".to_string(),
        contact_id: "contact-1".to_string(),
        update_type: "card_update".to_string(),
        payload: b"sensitive payload data".to_vec(),
        created_at: 1000,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };

    storage.pending().queue_update(&update).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT payload_encrypted FROM pending_updates WHERE id = ?1",
            ["upd-1"],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted payload should exist");
    assert!(!blob.is_empty());

    assert!(
        !String::from_utf8_lossy(&blob).contains("sensitive payload"),
        "Encrypted blob should not contain plaintext"
    );
}

// === Retry Entries Encryption ===

// @internal
#[test]
fn test_retry_entry_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let entry = RetryEntry {
        message_id: "msg-1".to_string(),
        recipient_id: "recipient-1".to_string(),
        payload: b"retry payload data".to_vec(),
        attempt: 1,
        next_retry: 2000,
        created_at: 1000,
        max_attempts: 10,
    };

    storage.retries().create_retry_entry(&entry).unwrap();

    let loaded = storage
        .retries()
        .get_retry_entry("msg-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.payload, b"retry payload data");
    assert_eq!(loaded.recipient_id, "recipient-1");
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_retry_entry_payload_stored_encrypted() {
    let (dir, storage) = open_storage();

    let entry = RetryEntry {
        message_id: "msg-1".to_string(),
        recipient_id: "recipient-1".to_string(),
        payload: b"retry payload data".to_vec(),
        attempt: 1,
        next_retry: 2000,
        created_at: 1000,
        max_attempts: 10,
    };

    storage.retries().create_retry_entry(&entry).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT payload_encrypted FROM retry_entries WHERE message_id = ?1",
            ["msg-1"],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted payload should exist");
    assert!(!blob.is_empty());
}

// === Device Sync Checkpoints Encryption ===

// @internal
#[test]
fn test_device_sync_checkpoint_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let target_device_id = [0xAA; 32];
    let items = vec![]; // Empty items list for simplicity
    storage
        .sync()
        .save_sync_checkpoint(&target_device_id, &items, 0)
        .unwrap();

    let loaded = storage
        .sync()
        .load_sync_checkpoint(&target_device_id)
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.0.len(), 0);
    assert_eq!(loaded.1, 0);
}

// @internal
#[test]
fn test_device_sync_checkpoint_stored_encrypted() {
    let (dir, storage) = open_storage();

    let target_device_id = [0xAA; 32];
    storage
        .sync()
        .save_sync_checkpoint(&target_device_id, &[], 5)
        .unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT items_json_encrypted FROM device_sync_checkpoints WHERE target_device_id = ?1",
            rusqlite::params![target_device_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    // Plaintext should be cleared
    let json: String = raw_conn
        .query_row(
            "SELECT items_json FROM device_sync_checkpoints WHERE target_device_id = ?1",
            rusqlite::params![target_device_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(json, "", "Plaintext items_json should be cleared");
}

// === Recovery Responses Encryption ===

// @internal
#[test]
fn test_recovery_response_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    storage
        .recovery()
        .save_recovery_response("claim-1", "contact-1", "accepted", Some(5000))
        .unwrap();

    let loaded = storage
        .recovery()
        .get_recovery_response("claim-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.0, "contact-1");
    assert_eq!(loaded.1, "accepted");
    assert_eq!(loaded.2, Some(5000));
}

// @internal
#[test]
fn test_recovery_response_stored_encrypted() {
    let (dir, storage) = open_storage();

    storage
        .recovery()
        .save_recovery_response("claim-1", "contact-1", "accepted", Some(5000))
        .unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT response_encrypted FROM recovery_responses WHERE claim_id = ?1",
            ["claim-1"],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    // Plaintext response column should be cleared
    let response: String = raw_conn
        .query_row(
            "SELECT response FROM recovery_responses WHERE claim_id = ?1",
            ["claim-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(response, "", "Plaintext response should be cleared");
}

// === Deletion State Encryption ===

// @internal
#[test]
fn test_deletion_state_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    let state = DeletionState::Scheduled {
        scheduled_at: 1000,
        execute_at: 2000,
    };
    storage.consent().save_deletion_state(&state).unwrap();

    let loaded = storage.consent().load_deletion_state().unwrap();
    assert_eq!(loaded, state);
}

// @scenario: security :: Local database encryption
// @internal
#[test]
fn test_deletion_state_stored_encrypted() {
    let (dir, storage) = open_storage();

    let state = DeletionState::Scheduled {
        scheduled_at: 1000,
        execute_at: 2000,
    };
    storage.consent().save_deletion_state(&state).unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT state_json_encrypted FROM deletion_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    // Plaintext should be cleared
    let json: String = raw_conn
        .query_row(
            "SELECT state_json FROM deletion_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(json, "", "Plaintext state_json should be cleared");
}

// === Sync Checkpoints Encryption ===

// @internal
#[test]
fn test_sync_checkpoint_encrypted_roundtrip() {
    let (_dir, storage) = open_storage();

    storage
        .sync()
        .save_batch_checkpoint("batch-1", "batch-1", 10, 5, r#"{"step":"half"}"#)
        .unwrap();

    let loaded = storage
        .sync()
        .load_batch_checkpoint("batch-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.0, 10);
    assert_eq!(loaded.1, 5);
    assert_eq!(loaded.2, r#"{"step":"half"}"#);
}

// @internal
#[test]
fn test_sync_checkpoint_stored_encrypted() {
    let (dir, storage) = open_storage();

    storage
        .sync()
        .save_batch_checkpoint("batch-1", "batch-1", 10, 5, r#"{"step":"half"}"#)
        .unwrap();

    let db_path = dir.path().join("vauchi.db");
    let raw_conn = rusqlite::Connection::open(&db_path).unwrap();

    // Find the row by batch_id
    let result: Option<Vec<u8>> = raw_conn
        .query_row(
            "SELECT state_json_encrypted FROM sync_checkpoints WHERE batch_id = ?1",
            ["batch-1"],
            |row| row.get(0),
        )
        .unwrap();

    let blob = result.expect("Encrypted blob should exist");
    assert!(!blob.is_empty());

    // Plaintext should be cleared
    let json: String = raw_conn
        .query_row(
            "SELECT state_json FROM sync_checkpoints WHERE batch_id = ?1",
            ["batch-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(json, "", "Plaintext state_json should be cleared");
}

// === Rekey Tests ===

// @internal
#[test]
fn test_rekey_preserves_device_info() {
    let (_dir, mut storage) = open_storage();

    storage
        .device()
        .save_device_info(&[0x42; 32], 1, "RekeyDevice", 1000)
        .unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .device()
        .load_device_info()
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.0, [0x42; 32]);
    assert_eq!(loaded.2, "RekeyDevice");
}

// @internal
#[test]
fn test_rekey_preserves_version_vector() {
    let (_dir, mut storage) = open_storage();

    let mut vector = VersionVector::new();
    vector.increment(&[0x01; 32]);
    storage.sync().save_version_vector(&vector).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .sync()
        .load_version_vector()
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.get(&[0x01; 32]), vector.get(&[0x01; 32]));
}

// @internal
#[test]
fn test_rekey_preserves_pending_updates() {
    let (_dir, mut storage) = open_storage();

    let update = PendingUpdate {
        id: "upd-1".to_string(),
        contact_id: "contact-1".to_string(),
        update_type: "card_update".to_string(),
        payload: b"sensitive data".to_vec(),
        created_at: 1000,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };
    storage.pending().queue_update(&update).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .pending()
        .get_pending_update("upd-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.payload, b"sensitive data");
}

// @internal
#[test]
fn test_rekey_preserves_retry_entries() {
    let (_dir, mut storage) = open_storage();

    let entry = RetryEntry {
        message_id: "msg-1".to_string(),
        recipient_id: "recipient-1".to_string(),
        payload: b"retry data".to_vec(),
        attempt: 1,
        next_retry: 2000,
        created_at: 1000,
        max_attempts: 10,
    };
    storage.retries().create_retry_entry(&entry).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage
        .retries()
        .get_retry_entry("msg-1")
        .unwrap()
        .expect("Should exist");
    assert_eq!(loaded.payload, b"retry data");
}

// @internal
#[test]
fn test_rekey_preserves_deletion_state() {
    let (_dir, mut storage) = open_storage();

    let state = DeletionState::Scheduled {
        scheduled_at: 1000,
        execute_at: 2000,
    };
    storage.consent().save_deletion_state(&state).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    let loaded = storage.consent().load_deletion_state().unwrap();
    assert_eq!(loaded, state);
}

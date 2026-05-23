// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for StorageError disk-full detection and user messages.

use vauchi_core::storage::StorageError;

// @internal
#[test]
fn sqlite_full_error_code_13_converts_to_disk_full() {
    // SQLITE_FULL has extended_code = 13
    let ffi_err = rusqlite::ffi::Error::new(13);
    let sqlite_err = rusqlite::Error::SqliteFailure(ffi_err, Some("database is full".into()));
    let storage_err: StorageError = sqlite_err.into();
    assert!(
        matches!(storage_err, StorageError::DiskFull),
        "SQLITE_FULL (code 13) should map to DiskFull, got: {storage_err:?}"
    );
}

// @internal
#[test]
fn other_sqlite_errors_remain_database() {
    // SQLITE_BUSY has extended_code = 5
    let ffi_err = rusqlite::ffi::Error::new(5);
    let sqlite_err = rusqlite::Error::SqliteFailure(ffi_err, Some("database is locked".into()));
    let storage_err: StorageError = sqlite_err.into();
    assert!(
        matches!(storage_err, StorageError::Database(_)),
        "SQLITE_BUSY should remain Database, got: {storage_err:?}"
    );
}

// @internal
#[test]
fn disk_full_user_message_is_actionable() {
    let err = StorageError::DiskFull;
    let msg = err.user_message();
    assert!(msg.contains("storage is full"), "Message: {msg}");
    assert!(msg.contains("Free up space"), "Message: {msg}");
}

// @internal
#[test]
fn queue_full_user_message_mentions_sync() {
    let err = StorageError::QueueFull("test".into());
    let msg = err.user_message();
    assert!(msg.contains("sync"), "Message: {msg}");
}

// @internal
#[test]
fn generic_error_user_message_is_safe() {
    let err = StorageError::Encryption("internal detail".into());
    let msg = err.user_message();
    assert!(
        !msg.contains("internal"),
        "User message should not expose internal details"
    );
}

// ============================================================
// Replay-nonce row-corruption propagation
// (site 2 of _private/.../2026-05-21-silent-failures-in-security-paths)
//
// Pre-2026-05-23 `load_replay_nonces` discarded row read errors AND
// rows whose `nonce` BLOB was not 32 bytes via `.filter_map(|r| r.ok())`
// and `nonce_vec.try_into().ok()?`. A corrupted nonce row → empty set
// → ADR-029 replay defense window. The fix propagates both classes of
// error so storage faults surface loudly instead of opening a silent
// security hole.
// ============================================================

use vauchi_core::SymmetricKey;
use vauchi_core::storage::Storage;

// @internal
#[test]
fn load_replay_nonces_returns_err_when_nonce_blob_has_wrong_length() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Insert a malformed row directly: a 31-byte nonce (one byte short)
    // simulates either DB tampering or single-row corruption that the
    // current `nonce_vec.try_into().ok()?` silently filters out.
    storage
        .test_insert_malformed_replay_nonce("contact-1", &[0xAAu8; 31], 100)
        .expect("test helper insert should succeed");

    let result = storage.load_replay_nonces("contact-1");
    assert!(
        result.is_err(),
        "malformed replay-nonce row must surface as Err (ADR-029 replay defense), got {:?}",
        result
    );
}

// @internal
#[test]
fn load_replay_nonces_happy_path_returns_inserted_nonces_in_order() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .save_replay_nonce("contact-1", &[0x11u8; 32], 100)
        .unwrap();
    storage
        .save_replay_nonce("contact-1", &[0x22u8; 32], 200)
        .unwrap();

    let nonces = storage.load_replay_nonces("contact-1").unwrap();
    assert_eq!(nonces.len(), 2, "both inserted nonces should load");
    assert_eq!(nonces[0], ([0x11u8; 32], 100));
    assert_eq!(nonces[1], ([0x22u8; 32], 200));
}

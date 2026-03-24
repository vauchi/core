// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for StorageError disk-full detection and user messages.

use vauchi_core::storage::StorageError;

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

#[test]
fn disk_full_user_message_is_actionable() {
    let err = StorageError::DiskFull;
    let msg = err.user_message();
    assert!(msg.contains("storage is full"), "Message: {msg}");
    assert!(msg.contains("Free up space"), "Message: {msg}");
}

#[test]
fn queue_full_user_message_mentions_sync() {
    let err = StorageError::QueueFull("test".into());
    let msg = err.user_message();
    assert!(msg.contains("sync"), "Message: {msg}");
}

#[test]
fn generic_error_user_message_is_safe() {
    let err = StorageError::Encryption("internal detail".into());
    let msg = err.user_message();
    assert!(
        !msg.contains("internal"),
        "User message should not expose internal details"
    );
}

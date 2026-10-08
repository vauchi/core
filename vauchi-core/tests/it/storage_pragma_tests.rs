// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SQLite PRAGMA configuration tests.
//!
//! Verifies that Storage applies performance-critical PRAGMAs on open.
//! Traces to: features/performance.feature @resources

use tempfile::NamedTempFile;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

/// WAL mode should be enabled for file-based storage.
// @internal
#[test]
fn test_wal_mode_enabled() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let mode: String = storage
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();

    assert_eq!(mode, "wal", "Expected WAL journal mode, got '{}'", mode);
}

/// synchronous should be set to NORMAL (1) for better write performance.
// @internal
#[test]
fn test_synchronous_normal() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let sync: i64 = storage
        .connection()
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .unwrap();

    assert_eq!(sync, 1, "Expected synchronous=NORMAL (1), got {}", sync);
}

/// cache_size should be configured for performance.
// @internal
#[test]
fn test_cache_size_configured() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let cache: i64 = storage
        .connection()
        .query_row("PRAGMA cache_size", [], |row| row.get(0))
        .unwrap();

    // Negative values mean KiB pages, positive mean page count.
    // We set 10000 (pages).
    assert_eq!(cache, 10000, "Expected cache_size=10000, got {}", cache);
}

/// In-memory storage should not crash when PRAGMAs are applied.
/// WAL is not supported for :memory: — SQLite silently falls back to "memory" journal mode.
// @internal
#[test]
fn test_in_memory_does_not_crash() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Just verify it opened successfully and we can query
    let version = storage.schema_version().unwrap();
    assert!(version > 0, "Schema version should be > 0 after migrations");
}

// ============================================================================
// Security PRAGMAs (crypto-shredding defense-in-depth)
// ============================================================================

/// secure_delete should be ON to overwrite deleted content with zeros.
// @internal
#[test]
fn test_secure_delete_enabled() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let secure_delete: i64 = storage
        .connection()
        .query_row("PRAGMA secure_delete", [], |row| row.get(0))
        .unwrap();

    assert_eq!(
        secure_delete, 1,
        "Expected secure_delete=ON (1), got {}",
        secure_delete
    );
}

/// temp_store should be MEMORY (2) to keep temporary tables in RAM.
// @internal
#[test]
fn test_temp_store_memory() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let temp_store: i64 = storage
        .connection()
        .query_row("PRAGMA temp_store", [], |row| row.get(0))
        .unwrap();

    assert_eq!(
        temp_store, 2,
        "Expected temp_store=MEMORY (2), got {}",
        temp_store
    );
}

/// auto_vacuum should be FULL (1) for new databases.
// @internal
#[test]
fn test_auto_vacuum_full() {
    let tmp = NamedTempFile::new().unwrap();
    let storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();

    let auto_vacuum: i64 = storage
        .connection()
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();

    assert_eq!(
        auto_vacuum, 1,
        "Expected auto_vacuum=FULL (1), got {}",
        auto_vacuum
    );
}

// ============================================================================
// ============================================================================

/// Database file should be created with 0600 permissions (owner-only).
#[cfg(unix)]
// @internal
#[test]
fn test_database_file_permissions_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    // File must not exist before Storage::open creates it
    assert!(!db_path.exists());

    let _storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();

    let perms = std::fs::metadata(&db_path).unwrap().permissions();
    let mode = perms.mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "Database file should have 0600 permissions, got {:o}",
        mode
    );
}

/// Existing database files should not have their permissions changed on reopen.
#[cfg(unix)]
// @internal
#[test]
fn test_existing_database_permissions_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let key = SymmetricKey::generate();
    {
        let _storage = Storage::open(&db_path, key.clone()).unwrap();
    }

    // Manually widen permissions (simulate user choice)
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Reopen — should NOT reset permissions
    let _storage = Storage::open(&db_path, key).unwrap();

    let perms = std::fs::metadata(&db_path).unwrap().permissions();
    let mode = perms.mode() & 0o777;
    assert_eq!(
        mode, 0o644,
        "Existing file permissions should be preserved, got {:o}",
        mode
    );
}

// ============================================================================
// Display name index tests (Migration V12)
// ============================================================================

// Names are decrypted before sorting and matching (#570), so search and
// paging cost one decryption per contact: measured 37 ms (release) and
// 257 ms (debug) for one search plus one page. The 1 s bound catches an
// algorithmic regression without flaking on a slow debug runner.
// @internal
#[test]
fn search_and_paging_over_1000_encrypted_contacts_stay_interactive() {
    use std::time::Instant;
    use vauchi_core::ContactCard;
    use vauchi_core::contact::Contact;

    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    for i in 0..1000u32 {
        let mut public_key = [0u8; 32];
        public_key[..4].copy_from_slice(&i.to_be_bytes());
        let contact = Contact::from_exchange(
            public_key,
            ContactCard::new(&format!("User {i:04}")),
            SymmetricKey::generate(),
            0,
        );
        storage.contacts().save_contact(&contact).unwrap();
    }

    let start = Instant::now();
    let found = storage.contacts().search_contacts("user 05").unwrap();
    let page = storage.contacts().list_contacts_paginated(500, 20).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(found.len(), 100);
    assert_eq!(page.first().map(Contact::display_name), Some("User 0500"));
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "search + one page took {elapsed:?}, expected < 1s"
    );
}

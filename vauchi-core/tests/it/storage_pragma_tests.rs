// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SQLite PRAGMA configuration tests.
//!
//! Verifies that Storage applies performance-critical PRAGMAs on open.
//! Traces to: features/performance.feature @resources

use crate::common;

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

    {
        let _storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    }

    // Manually widen permissions (simulate user choice)
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Reopen — should NOT reset permissions
    let _storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();

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

/// The contacts display_name index should exist after migrations.
// @internal
#[test]
fn test_display_name_index_exists() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let index_exists: bool = storage
        .connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_contacts_display_name'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        index_exists,
        "Expected idx_contacts_display_name index to exist"
    );
}

/// Searching 1000 contacts by display_name should complete in under 200ms.
// @internal
#[test]
fn test_search_1000_contacts_under_200ms() {
    use std::time::Instant;

    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // We use raw SQL to avoid needing encryption for card/key fields
    // (they're BLOBs, so dummy data works for index benchmarking).
    let conn = storage.connection();
    for i in 0..1000 {
        conn.execute(
            "INSERT INTO contacts (id, public_key, display_name, card_encrypted, shared_key_encrypted, exchange_timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                format!("contact-{:04}", i),
                vec![i as u8; 32],
                format!("User {:04}", i),
                vec![0u8; 64],
                vec![0u8; 64],
                1000 + i as i64,
            ],
        ).unwrap();
    }

    let start = Instant::now();

    // Search by display_name prefix (case-insensitive via COLLATE NOCASE index)
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM contacts WHERE display_name LIKE 'User 05%'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let elapsed = start.elapsed();

    assert!(count > 0, "Search should find matching contacts");
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "Search took {:?}, expected < 200ms",
        elapsed
    );
}

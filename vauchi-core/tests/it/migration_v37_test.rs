// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Migration v37 test: contact delete/archive columns.
//!
//! Verifies that the `contacts` table has the expected new columns
//! with correct types and default values after running all migrations.

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

/// Opens a fresh in-memory Storage (runs all migrations) and verifies
/// that the three new columns exist with correct defaults.
#[test]
fn migration_v37_adds_delete_archive_columns() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();
    let conn = storage.connection();

    // Query PRAGMA for column info
    let columns: Vec<(String, String, i32, Option<String>)> = {
        let mut stmt = conn.prepare("PRAGMA table_info(contacts)").unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,         // name
                row.get::<_, String>(2)?,         // type
                row.get::<_, i32>(3)?,            // notnull
                row.get::<_, Option<String>>(4)?, // dflt_value
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    // Helper to find column by name
    let find_col = |name: &str| -> (String, String, i32, Option<String>) {
        columns
            .iter()
            .find(|(n, _, _, _)| n == name)
            .cloned()
            .unwrap_or_else(|| panic!("Column '{}' not found in contacts table", name))
    };

    // deleted_at: INTEGER, nullable (notnull=0), default NULL
    let (_, dtype, notnull, dflt) = find_col("deleted_at");
    assert_eq!(dtype, "INTEGER", "deleted_at should be INTEGER");
    assert_eq!(notnull, 0, "deleted_at should be nullable");
    assert!(
        dflt.is_none() || dflt.as_deref() == Some("NULL"),
        "deleted_at default should be NULL, got {:?}",
        dflt
    );

    // archived: INTEGER, NOT NULL, default 0
    let (_, dtype, notnull, dflt) = find_col("archived");
    assert_eq!(dtype, "INTEGER", "archived should be INTEGER");
    assert_eq!(notnull, 1, "archived should be NOT NULL");
    assert_eq!(dflt.as_deref(), Some("0"), "archived default should be 0");

    // archived_at: INTEGER, nullable (notnull=0), default NULL
    let (_, dtype, notnull, dflt) = find_col("archived_at");
    assert_eq!(dtype, "INTEGER", "archived_at should be INTEGER");
    assert_eq!(notnull, 0, "archived_at should be nullable");
    assert!(
        dflt.is_none() || dflt.as_deref() == Some("NULL"),
        "archived_at default should be NULL, got {:?}",
        dflt
    );
}

/// Verifies that the schema version is at least 37 after opening storage.
#[test]
fn migration_v37_schema_version_is_at_least_37() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();
    let version = storage.schema_version().unwrap();
    assert!(
        version >= 37,
        "Schema version should be >= 37, got {}",
        version
    );
}

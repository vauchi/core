// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Migration v67: the per-contact ignore columns (ADR-072).

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn contact_column(name: &str) -> (String, i32, Option<String>) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let conn = storage.connection();
    let mut stmt = conn.prepare("PRAGMA table_info(contacts)").unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i32>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })
    .unwrap()
    .filter_map(Result::ok)
    .find(|(n, _, _, _)| n == name)
    .map(|(_, dtype, notnull, dflt)| (dtype, notnull, dflt))
    .unwrap_or_else(|| panic!("column '{name}' not found in contacts table"))
}

// @internal
#[test]
fn migration_v67_adds_not_null_ignored_flag_defaulting_to_zero() {
    let (dtype, notnull, dflt) = contact_column("ignored");
    assert_eq!(dtype, "INTEGER");
    assert_eq!(notnull, 1, "ignored must be NOT NULL");
    assert_eq!(dflt.as_deref(), Some("0"), "ignored must default to 0");
}

// @internal
#[test]
fn migration_v67_adds_nullable_ignored_at() {
    let (dtype, notnull, dflt) = contact_column("ignored_at");
    assert_eq!(dtype, "INTEGER");
    assert_eq!(notnull, 0, "ignored_at must be nullable");
    assert!(
        dflt.is_none() || dflt.as_deref() == Some("NULL"),
        "ignored_at must default to NULL, got {dflt:?}"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identifying values are not stored in plaintext columns
//! (vauchi/private#579).

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn columns(storage: &Storage, table: &str) -> Vec<String> {
    let mut stmt = storage
        .connection()
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    stmt.query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

// The name is already inside the encrypted identity blob.
// @internal
#[test]
fn the_identity_table_keeps_no_display_name() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    assert!(!columns(&storage, "identity").contains(&"display_name".to_string()));
}

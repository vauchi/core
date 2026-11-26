// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::crypto::SymmetricKey;
use crate::storage::{NoticeStateKind, Storage, StorageError};
use rusqlite::Connection;

fn sealed_profile(path: &std::path::Path) -> Storage {
    let storage = Storage::open(path, SymmetricKey::generate()).unwrap();
    drop(storage);
    let target = Connection::open(path).unwrap();
    Storage::configure_pragmas(&target).unwrap();
    Storage::empty_mask_profile(SymmetricKey::generate(), Some(target)).unwrap()
}

fn save(storage: &Storage) -> Result<(), StorageError> {
    storage
        .notices()
        .save("contact", NoticeStateKind::Sender, &[1; 32], b"state")
}

// @internal
#[test]
fn a_sealed_notice_transaction_commits_its_actual_file_at_full_sync() {
    let directory = tempfile::tempdir().unwrap();
    let storage = sealed_profile(&directory.path().join("vauchi.db"));
    let target = storage.seal_target.as_ref().unwrap();
    target
        .execute_batch(
            "CREATE TABLE observed_seal(level INTEGER);
         CREATE TRIGGER observe_insert AFTER INSERT ON duress_content BEGIN
             INSERT INTO observed_seal SELECT synchronous FROM pragma_synchronous;
         END;
         CREATE TRIGGER observe_update AFTER UPDATE ON duress_content BEGIN
             INSERT INTO observed_seal SELECT synchronous FROM pragma_synchronous;
         END;",
        )
        .unwrap();
    let receipt = storage
        .with_notice_transaction(|| {
            save(&storage)?;
            Ok::<_, StorageError>("durable")
        })
        .unwrap();
    assert_eq!(receipt, "durable");
    let observed: i64 = target
        .query_row("SELECT min(level) FROM observed_seal", [], |row| row.get(0))
        .unwrap();
    assert!(
        observed >= 2,
        "actual sealed-row commit used synchronous={observed}"
    );
    assert_eq!(
        target
            .pragma_query_value::<i64, _>(None, "synchronous", |row| row.get(0))
            .unwrap(),
        1
    );
}

// @internal
#[test]
fn a_sealed_notice_receipt_never_escapes_an_uncommitted_file_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let storage = sealed_profile(&directory.path().join("vauchi.db"));
    let target = storage.seal_target.as_ref().unwrap();
    target.pragma_update(None, "synchronous", 2).unwrap();
    target.execute_batch("BEGIN IMMEDIATE").unwrap();
    let receipt = storage.with_notice_transaction(|| {
        save(&storage)?;
        Ok::<_, StorageError>("must not leave")
    });
    assert!(matches!(receipt, Err(StorageError::InvalidData(_))));
    target.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        storage
            .notices()
            .load("contact", NoticeStateKind::Sender, &[1; 32])
            .unwrap(),
        None
    );
}

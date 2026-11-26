// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::crypto::SymmetricKey;
use crate::storage::{NoticeStateKind, NoticeWrite, Storage, StorageError};
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

// @internal
#[test]
fn independently_opened_mask_profiles_cannot_both_advance_one_observation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vauchi.db");
    let initial = sealed_profile(&path);
    let key = initial.encryption_key.clone();
    initial.with_notice_transaction(|| save(&initial)).unwrap();
    drop(initial);
    let first = Storage::open_mask_profile(&path, key.clone()).unwrap();
    let second = Storage::open_mask_profile(&path, key.clone()).unwrap();
    let before_first = first
        .notices()
        .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
        .unwrap();
    let before_second = second
        .notices()
        .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
        .unwrap();
    assert_eq!(before_first.state(), Some(b"state".as_slice()));
    assert_eq!(before_second.state(), before_first.state());
    first
        .notices()
        .save_many(&[NoticeWrite::new(&before_first, b"first")])
        .unwrap();
    let refused = second
        .notices()
        .save_many(&[NoticeWrite::new(&before_second, b"stale")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    let reopened = Storage::open_mask_profile(&path, key).unwrap();
    let held = reopened
        .notices()
        .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
        .unwrap();
    assert_eq!(held.state(), Some(b"first".as_slice()));
    reopened
        .notices()
        .save_many(&[NoticeWrite::new(&held, b"recovered")])
        .unwrap();
    assert_eq!(
        reopened
            .notices()
            .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
            .unwrap()
            .state(),
        Some(b"recovered".as_slice())
    );
}

// @internal
#[test]
fn a_stale_embedded_mask_cannot_replace_a_lent_profiles_notice_commit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let mut owner = Storage::open(&path, key.clone()).unwrap();
    let stale = Storage::open(&path, key).unwrap();
    let live = owner.lend_mask_profile().unwrap();
    let observed = live
        .notices()
        .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
        .unwrap();
    live.notices()
        .save_many(&[NoticeWrite::new(&observed, b"advanced")])
        .unwrap();
    assert!(matches!(stale.seal_mask(), Err(StorageError::NoticeMoved)));
    owner.take_back_mask_profile(live).unwrap();
    let held = owner
        .mask_profile()
        .notices()
        .snapshot("contact", NoticeStateKind::Sender, &[1; 32])
        .unwrap();
    assert_eq!(held.state(), Some(b"advanced".as_slice()));
}

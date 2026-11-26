// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A persisted notice session cannot partly advance its directions.
#![cfg(feature = "notice-core")]

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{NoticeStateKind, NoticeWrite, Storage, StorageError};

const DEVICE: [u8; 32] = [1; 32];

fn write(kind: NoticeStateKind, state: &[u8]) -> NoticeWrite<'_> {
    NoticeWrite::new("contact", kind, DEVICE, state)
}

fn load(storage: &Storage, kind: NoticeStateKind) -> Option<Vec<u8>> {
    storage
        .notices()
        .load("contact", kind, &DEVICE)
        .unwrap()
        .map(|bytes| bytes.to_vec())
}

// @internal
#[test]
fn a_batch_commits_every_direction_and_failed_commit_keeps_the_previous_session() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .notices()
        .save_many(&[
            write(NoticeStateKind::Sender, b"sender-0"),
            write(NoticeStateKind::Receiver, b"receiver-0"),
        ])
        .unwrap();
    storage.arm_commit_fault();
    let failed = storage.notices().save_many(&[
        write(NoticeStateKind::Sender, b"sender-1"),
        write(NoticeStateKind::Receiver, b"receiver-1"),
    ]);
    assert!(matches!(failed, Err(StorageError::Database(_))));
    assert_eq!(
        load(&storage, NoticeStateKind::Sender),
        Some(b"sender-0".to_vec())
    );
    assert_eq!(
        load(&storage, NoticeStateKind::Receiver),
        Some(b"receiver-0".to_vec())
    );
    storage
        .notices()
        .save_many(&[
            write(NoticeStateKind::Sender, b"sender-1"),
            write(NoticeStateKind::Receiver, b"receiver-1"),
        ])
        .unwrap();
    assert_eq!(
        load(&storage, NoticeStateKind::Sender),
        Some(b"sender-1".to_vec())
    );
    assert_eq!(
        load(&storage, NoticeStateKind::Receiver),
        Some(b"receiver-1".to_vec())
    );
}

// @internal
#[test]
fn one_stale_direction_refuses_the_whole_batch_without_advancing_another() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let first = Storage::open(&path, key.clone()).unwrap();
    first
        .notices()
        .save_many(&[
            write(NoticeStateKind::Sender, b"sender-0"),
            write(NoticeStateKind::Receiver, b"receiver-0"),
        ])
        .unwrap();
    let second = Storage::open(&path, key).unwrap();
    assert_eq!(
        load(&second, NoticeStateKind::Sender),
        Some(b"sender-0".to_vec())
    );
    assert_eq!(
        load(&second, NoticeStateKind::Receiver),
        Some(b"receiver-0".to_vec())
    );
    first
        .notices()
        .save_many(&[write(NoticeStateKind::Sender, b"sender-first")])
        .unwrap();
    let failed = second.notices().save_many(&[
        write(NoticeStateKind::Receiver, b"receiver-stale"),
        write(NoticeStateKind::Sender, b"sender-stale"),
    ]);
    assert!(matches!(failed, Err(StorageError::NoticeMoved)));
    assert_eq!(
        load(&second, NoticeStateKind::Sender),
        Some(b"sender-first".to_vec())
    );
    assert_eq!(
        load(&second, NoticeStateKind::Receiver),
        Some(b"receiver-0".to_vec())
    );
}

// @internal
#[test]
fn an_absent_read_cannot_overwrite_a_direction_another_connection_opened() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let first = Storage::open(&path, key.clone()).unwrap();
    let second = Storage::open(&path, key).unwrap();
    assert_eq!(load(&second, NoticeStateKind::Sender), None);
    first
        .notices()
        .save_many(&[write(NoticeStateKind::Sender, b"advanced")])
        .unwrap();
    let refused = second.notices().save_many(&[
        write(NoticeStateKind::Receiver, b"tentative"),
        write(NoticeStateKind::Sender, b"reopened"),
    ]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(
        load(&first, NoticeStateKind::Sender),
        Some(b"advanced".to_vec())
    );
    assert_eq!(load(&first, NoticeStateKind::Receiver), None);
}

// @internal
#[test]
fn a_stale_writer_cannot_resurrect_a_direction_erased_with_its_contact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let first = Storage::open(&path, key.clone()).unwrap();
    first
        .notices()
        .save_many(&[write(NoticeStateKind::Sender, b"advanced")])
        .unwrap();
    let second = Storage::open(&path, key).unwrap();
    assert_eq!(
        load(&second, NoticeStateKind::Sender),
        Some(b"advanced".to_vec())
    );
    first.delete_contact("contact").unwrap();
    assert_eq!(load(&first, NoticeStateKind::Sender), None);
    let refused = second
        .notices()
        .save_many(&[write(NoticeStateKind::Sender, b"resurrected")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&first, NoticeStateKind::Sender), None);
}

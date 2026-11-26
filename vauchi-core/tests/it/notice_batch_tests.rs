// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A persisted notice session cannot partly advance or overwrite another snapshot.
#![cfg(feature = "notice-core")]

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{NoticeSnapshot, NoticeStateKind, NoticeWrite, Storage, StorageError};

#[path = "notice_snapshot_tests.rs"]
mod snapshots;

const DEVICE: [u8; 32] = [1; 32];
const SENDER: NoticeStateKind = NoticeStateKind::Sender;
const RECEIVER: NoticeStateKind = NoticeStateKind::Receiver;

fn snapshot(storage: &Storage, kind: NoticeStateKind) -> NoticeSnapshot {
    storage
        .notices()
        .snapshot("contact", kind, &DEVICE)
        .unwrap()
}

fn load(storage: &Storage, kind: NoticeStateKind) -> Option<Vec<u8>> {
    snapshot(storage, kind).state().map(<[u8]>::to_vec)
}

fn save(storage: &Storage, states: &[(NoticeStateKind, &[u8])]) -> Result<(), StorageError> {
    let snapshots: Vec<_> = states
        .iter()
        .map(|(kind, _)| snapshot(storage, *kind))
        .collect();
    let writes: Vec<_> = snapshots
        .iter()
        .zip(states)
        .map(|(snapshot, (_, state))| NoticeWrite::new(snapshot, state))
        .collect();
    storage.notices().save_many(&writes)
}

struct Connections {
    _directory: tempfile::TempDir,
    first: Storage,
    second: Storage,
}

impl Connections {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vauchi.db");
        let key = SymmetricKey::generate();
        let first = Storage::open(&path, key.clone()).unwrap();
        let second = Storage::open(&path, key).unwrap();
        Self {
            _directory: directory,
            first,
            second,
        }
    }
}

// @internal
#[test]
fn a_batch_commits_every_direction_and_failed_commit_keeps_the_previous_session() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    save(
        &storage,
        &[(SENDER, b"sender-0"), (RECEIVER, b"receiver-0")],
    )
    .unwrap();
    storage.arm_commit_fault();
    let failed = save(
        &storage,
        &[(SENDER, b"sender-1"), (RECEIVER, b"receiver-1")],
    );
    assert!(matches!(failed, Err(StorageError::Database(_))));
    assert_eq!(load(&storage, SENDER), Some(b"sender-0".to_vec()));
    assert_eq!(load(&storage, RECEIVER), Some(b"receiver-0".to_vec()));
    save(
        &storage,
        &[(SENDER, b"sender-1"), (RECEIVER, b"receiver-1")],
    )
    .unwrap();
    assert_eq!(load(&storage, SENDER), Some(b"sender-1".to_vec()));
    assert_eq!(load(&storage, RECEIVER), Some(b"receiver-1".to_vec()));
}

// @internal
#[test]
fn one_stale_direction_refuses_the_whole_batch_without_advancing_another() {
    let Connections {
        _directory,
        first,
        second,
    } = Connections::new();
    save(&first, &[(SENDER, b"sender-0"), (RECEIVER, b"receiver-0")]).unwrap();
    let sender = snapshot(&second, SENDER);
    let receiver = snapshot(&second, RECEIVER);
    assert_eq!(sender.state(), Some(b"sender-0".as_slice()));
    assert_eq!(receiver.state(), Some(b"receiver-0".as_slice()));
    save(&first, &[(SENDER, b"sender-first")]).unwrap();
    let failed = second.notices().save_many(&[
        NoticeWrite::new(&receiver, b"receiver-stale"),
        NoticeWrite::new(&sender, b"sender-stale"),
    ]);
    assert!(matches!(failed, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&second, SENDER), Some(b"sender-first".to_vec()));
    assert_eq!(load(&second, RECEIVER), Some(b"receiver-0".to_vec()));
}

// @internal
#[test]
fn an_absent_read_cannot_overwrite_a_direction_another_connection_opened() {
    let Connections {
        _directory,
        first,
        second,
    } = Connections::new();
    let sender = snapshot(&second, SENDER);
    let receiver = snapshot(&second, RECEIVER);
    assert_eq!(sender.state(), None);
    save(&first, &[(SENDER, b"advanced")]).unwrap();
    let refused = second.notices().save_many(&[
        NoticeWrite::new(&receiver, b"tentative"),
        NoticeWrite::new(&sender, b"reopened"),
    ]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&first, SENDER), Some(b"advanced".to_vec()));
    assert_eq!(load(&first, RECEIVER), None);
}

// @internal
#[test]
fn a_stale_writer_cannot_resurrect_a_direction_erased_with_its_contact() {
    let Connections {
        _directory,
        first,
        second,
    } = Connections::new();
    save(&first, &[(SENDER, b"advanced")]).unwrap();
    let sender = snapshot(&second, SENDER);
    assert_eq!(sender.state(), Some(b"advanced".as_slice()));
    first.delete_contact("contact").unwrap();
    assert_eq!(load(&first, SENDER), None);
    let refused = second
        .notices()
        .save_many(&[NoticeWrite::new(&sender, b"resurrected")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&first, SENDER), None);
}

// @internal
#[test]
fn a_deleted_and_recreated_direction_does_not_match_an_old_snapshot() {
    let Connections {
        _directory,
        first,
        second,
    } = Connections::new();
    save(&first, &[(SENDER, b"initial")]).unwrap();
    let sender = snapshot(&second, SENDER);
    assert_eq!(sender.state(), Some(b"initial".as_slice()));
    save(&first, &[(SENDER, b"advanced")]).unwrap();
    first.delete_contact("contact").unwrap();
    assert_eq!(load(&first, SENDER), None);
    save(&first, &[(SENDER, b"fresh")]).unwrap();
    let refused = second
        .notices()
        .save_many(&[NoticeWrite::new(&sender, b"stale")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&first, SENDER), Some(b"fresh".to_vec()));
}

// @internal
#[test]
fn a_connection_that_never_read_a_direction_cannot_replace_it() {
    let Connections {
        _directory,
        first,
        second,
    } = Connections::new();
    save(&first, &[(SENDER, b"advanced")]).unwrap();
    let refused = second
        .notices()
        .save("contact", SENDER, &DEVICE, b"unobserved");
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&first, SENDER), Some(b"advanced".to_vec()));
}

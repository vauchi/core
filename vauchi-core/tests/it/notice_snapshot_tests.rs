// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use proptest::prelude::*;

// @internal
#[test]
fn two_snapshots_on_one_connection_cannot_both_advance_the_same_state() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    save(&storage, &[(SENDER, b"original")]).unwrap();
    let old = snapshot(&storage, SENDER);
    let current = snapshot(&storage, SENDER);
    storage
        .notices()
        .save_many(&[NoticeWrite::new(&current, b"new")])
        .unwrap();
    let refused = storage
        .notices()
        .save_many(&[NoticeWrite::new(&old, b"stale")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(old.state(), Some(b"original".as_slice()));
    assert_eq!(load(&storage, SENDER), Some(b"new".to_vec()));
}

// @internal
#[test]
fn two_missing_snapshots_on_one_connection_cannot_both_create_a_direction() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let first = snapshot(&storage, SENDER);
    let second = snapshot(&storage, SENDER);
    assert_eq!(first.state(), None);
    storage
        .notices()
        .save_many(&[NoticeWrite::new(&first, b"initial")])
        .unwrap();
    let refused = storage
        .notices()
        .save_many(&[NoticeWrite::new(&second, b"reopened")]);
    assert!(matches!(refused, Err(StorageError::NoticeMoved)));
    assert_eq!(load(&storage, SENDER), Some(b"initial".to_vec()));
}

proptest! {
    // @internal
    #[test]
    fn any_history_of_snapshots_advances_only_the_state_actually_observed(
        operations in prop::collection::vec((any::<bool>(), any::<usize>()), 1..48)
    ) {
        let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
        let mut saved = Vec::new();
        let mut revision = 0u32;
        let mut expected = None;
        for (read, choose) in operations {
            if read || saved.is_empty() {
                let observed = snapshot(&storage, SENDER);
                prop_assert_eq!(observed.state().map(<[u8]>::to_vec), expected.clone());
                saved.push((observed, revision));
            } else {
                let (observed, observed_revision) = &saved[choose % saved.len()];
                let next = (revision + 1).to_be_bytes();
                let result = storage.notices().save_many(&[NoticeWrite::new(observed, &next)]);
                if *observed_revision == revision {
                    result.unwrap();
                    revision += 1;
                    expected = Some(next.to_vec());
                } else {
                    prop_assert!(matches!(result, Err(StorageError::NoticeMoved)));
                }
                prop_assert_eq!(load(&storage, SENDER), expected.clone());
            }
        }
    }
}

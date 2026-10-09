// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A panic shred purges the current identity's relay data, never an earlier
//! identity's, even when an earlier identity's pre-signed messages are still
//! on disk (vauchi/private#582).

use vauchi_core::api::{
    PreSignedPurgeRequest, PreSignedShredMessages, PurgeSender, ShredError, ShredManager,
};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, Storage};

#[derive(Default)]
struct RecordingPurge {
    sent: Vec<[u8; 32]>,
}

impl PurgeSender for RecordingPurge {
    fn send_purge(&mut self, purge: &PreSignedPurgeRequest, _now: u64) -> Result<bool, ShredError> {
        self.sent.push(purge.public_key);
        Ok(true)
    }
}

fn panic_shred_purges(dir: &tempfile::TempDir, identity: &Identity) -> Vec<[u8; 32]> {
    let storage = Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let secure = MemoryKeyStorage::new();
    let mut purge = RecordingPurge::default();
    ShredManager::new(&storage, &secure, identity, dir.path())
        .panic_shred(Some(&mut purge), None)
        .unwrap();
    purge.sent
}

// @internal
#[test]
fn a_panic_shred_never_sends_an_earlier_identitys_purge() {
    let dir = tempfile::tempdir().unwrap();
    let earlier = Identity::create("Earlier", 0);
    PreSignedShredMessages::generate(&earlier, 0)
        .save(dir.path())
        .unwrap();
    let current = Identity::create("Current", 0);

    let sent = panic_shred_purges(&dir, &current);

    assert_eq!(sent, vec![*current.signing_public_key()]);
}

// @internal
#[test]
fn a_panic_shred_uses_the_current_identitys_stored_purge() {
    let dir = tempfile::tempdir().unwrap();
    let current = Identity::create("Current", 0);
    let stored = PreSignedShredMessages::generate(&current, 0);
    stored.save(dir.path()).unwrap();

    let sent = panic_shred_purges(&dir, &current);

    assert_eq!(sent, vec![stored.purge_request.public_key]);
}

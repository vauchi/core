// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The durable receive checkpoint: the last day a device read a relay's
//! mailboxes to exhaustion, scoped to that relay, identity and device
//! (vauchi/private#691).

use vauchi_core::Storage;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{ReceiveCheckpoint, ReceiveScope};

fn storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn scope(relay: &str, identity: u8, device: u8) -> ReceiveScope {
    ReceiveScope::new(relay, &[identity; 32], &[device; 32])
}

fn at(day: u64) -> ReceiveCheckpoint {
    ReceiveCheckpoint {
        day,
        resync_owed: false,
    }
}

// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_checkpoint_round_trips_and_the_latest_save_wins() {
    let storage = storage();
    let here = scope("https://relay.example", 1, 1);

    assert_eq!(storage.sync().load_receive_checkpoint(&here).unwrap(), None);

    storage
        .sync()
        .save_receive_checkpoint(&here, &at(19_000))
        .unwrap();
    assert_eq!(
        storage.sync().load_receive_checkpoint(&here).unwrap(),
        Some(at(19_000))
    );

    storage
        .sync()
        .save_receive_checkpoint(&here, &at(19_003))
        .unwrap();
    assert_eq!(
        storage.sync().load_receive_checkpoint(&here).unwrap(),
        Some(at(19_003))
    );
}

/// Another relay, identity or device has read nothing on this one's
/// behalf: each keeps its own checkpoint.
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_checkpoint_belongs_to_one_relay_identity_and_device() {
    let storage = storage();
    let here = scope("https://relay.example", 1, 1);
    storage
        .sync()
        .save_receive_checkpoint(&here, &at(19_000))
        .unwrap();

    for other in [
        scope("https://other.example", 1, 1),
        scope("https://relay.example", 2, 1),
        scope("https://relay.example", 1, 2),
    ] {
        assert_ne!(other, here);
        assert_eq!(
            storage.sync().load_receive_checkpoint(&other).unwrap(),
            None,
            "{other:?} must not see another scope's checkpoint"
        );
    }
    assert_eq!(scope("https://relay.example", 1, 1), here);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Authenticated own-device delivery through the live receive seam.

use super::*;
use crate::api::sync::SyncEnvelope;
use crate::crypto::SymmetricKey;
use crate::identity::DeviceRegistry;
use crate::storage::Storage;

const NOW: u64 = 1_000_000;

struct Pair {
    receiver: Vauchi,
    sender: Identity,
    storage: Storage,
    registry: DeviceRegistry,
}

impl Pair {
    fn new() -> Self {
        let mut receiver = Vauchi::in_memory().unwrap();
        receiver.create_identity("Owner").unwrap();
        let identity = receiver.identity().unwrap();
        let sender = Identity::from_device_link(
            *identity.master_seed(),
            "Owner".into(),
            1,
            "Tablet".into(),
            NOW,
        );
        let mut registry = identity.initial_device_registry();
        registry
            .add_device_unsigned(
                sender
                    .create_device_info(NOW)
                    .to_registered(sender.master_seed()),
            )
            .unwrap();
        receiver
            .storage()
            .device()
            .save_device_registry(&registry)
            .unwrap();
        Self {
            receiver,
            sender,
            storage: Storage::in_memory(SymmetricKey::generate()).unwrap(),
            registry,
        }
    }

    fn sender(&self) -> DeviceSyncOrchestrator<'_> {
        DeviceSyncOrchestrator::load(
            &self.storage,
            self.sender.create_device_info(NOW),
            self.registry.clone(),
        )
        .unwrap()
    }

    fn batch(&self, label: &str, value: &str) -> Vec<u8> {
        let mut sender = self.sender();
        sender
            .record_local_change(SyncItem::CardUpdated {
                field_label: label.into(),
                new_value: value.into(),
                timestamp: NOW,
            })
            .unwrap();
        let envelopes = sender.build_outbound_envelopes(&self.sender).unwrap();
        let envelope: SyncEnvelope = serde_json::from_slice(&envelopes[0]).unwrap();
        sender
            .mark_synced(
                self.receiver.identity().unwrap().device_id(),
                envelope.version,
            )
            .unwrap();
        envelope.ciphertext
    }

    fn apply(&self, ciphertext: &[u8]) -> usize {
        self.receiver
            .apply_device_sync_blob(self.receiver.identity().unwrap(), ciphertext)
            .unwrap()
    }

    fn value(&self, label: &str) -> String {
        self.receiver
            .storage()
            .contacts()
            .load_own_card()
            .unwrap()
            .unwrap()
            .fields()
            .iter()
            .find(|field| field.label() == label)
            .unwrap()
            .value()
            .into()
    }
}

// @internal
#[test]
fn own_device_order_rejects_reordered_same_second_write_after_reload() {
    let pair = Pair::new();
    let old = pair.batch("Email", "old@example.test");
    let new = pair.batch("Email", "new@example.test");
    assert_eq!(pair.apply(&new), 1);
    let replayed = pair.apply(&old);
    assert_eq!(pair.value("Email"), "new@example.test");
    assert_eq!(replayed, 0);
}

// @internal
#[test]
fn own_device_order_accepts_same_second_successor() {
    let pair = Pair::new();
    let old = pair.batch("Email", "old@example.test");
    let new = pair.batch("Email", "new@example.test");
    assert_eq!(pair.apply(&old), 1);
    assert_eq!(pair.apply(&new), 1);
    assert_eq!(pair.value("Email"), "new@example.test");
}

// @internal
#[test]
fn own_device_order_keeps_independent_conflict_keys() {
    let pair = Pair::new();
    let email = pair.batch("Email", "owner@example.test");
    let phone = pair.batch("Phone", "+41000000000");
    assert_eq!(pair.apply(&phone), 1);
    assert_eq!(pair.apply(&email), 1);
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert_eq!(pair.value("Phone"), "+41000000000");
}

impl Pair {
    fn plaintext(&self, ciphertext: &[u8]) -> Vec<u8> {
        self.sender()
            .decrypt_from_device(
                self.receiver
                    .identity()
                    .unwrap()
                    .create_device_info(NOW)
                    .exchange_public_key(),
                ciphertext,
            )
            .unwrap()
    }

    fn seal(&self, value: &serde_json::Value) -> Vec<u8> {
        self.sender()
            .encrypt_for_device(
                self.receiver
                    .identity()
                    .unwrap()
                    .create_device_info(NOW)
                    .exchange_public_key(),
                &serde_json::to_vec(value).unwrap(),
            )
            .unwrap()
    }
}

// @internal
#[test]
fn own_device_order_queue_retry_reload_and_partial_ack_keep_positions() {
    let pair = Pair::new();
    let mut sender = pair.sender();
    for value in ["old@example.test", "new@example.test"] {
        sender
            .record_local_change(SyncItem::CardUpdated {
                field_label: "Email".into(),
                new_value: value.into(),
                timestamp: NOW,
            })
            .unwrap();
    }
    let first = sender.build_outbound_envelopes(&pair.sender).unwrap();
    drop(sender);
    let mut reloaded = pair.sender();
    let retry = reloaded.build_outbound_envelopes(&pair.sender).unwrap();
    let extract = |bytes: &[u8]| {
        serde_json::from_slice::<SyncEnvelope>(bytes)
            .unwrap()
            .ciphertext
    };
    assert_eq!(
        pair.plaintext(&extract(&first[0])),
        pair.plaintext(&extract(&retry[0]))
    );
    assert_eq!(pair.apply(&extract(&first[0])), 2);
    assert_eq!(pair.apply(&extract(&retry[0])), 0);
    let target = pair.receiver.identity().unwrap().device_id();
    reloaded.mark_synced(target, 1).unwrap();
    assert_eq!(pair.sender().pending_for_device(target).len(), 1);
    let remaining = pair
        .sender()
        .build_outbound_envelopes(&pair.sender)
        .unwrap();
    let raw: serde_json::Value =
        serde_json::from_slice(&pair.plaintext(&extract(&remaining[0]))).unwrap();
    assert_eq!(
        raw[0]["_VauchiDeviceSyncOrder"]["sequences"],
        serde_json::json!([2])
    );
    assert_eq!(pair.apply(&extract(&remaining[0])), 0);
}

#[path = "device_sync_order_chunk_tests.rs"]
mod chunk_tests;

#[path = "device_sync_order_atomic_tests.rs"]
mod atomic_tests;

#[path = "device_sync_order_sender_tests.rs"]
mod sender_tests;

#[path = "device_sync_order_state_tests.rs"]
mod state_tests;

#[path = "device_sync_order_property_tests.rs"]
mod property_tests;

#[path = "device_sync_order_metadata_tests.rs"]
mod metadata_tests;

#[path = "device_sync_order_registry_fault_tests.rs"]
mod registry_fault_tests;

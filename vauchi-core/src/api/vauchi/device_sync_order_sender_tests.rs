// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn email(value: &str) -> SyncItem {
    SyncItem::CardUpdated {
        field_label: "Email".into(),
        new_value: value.into(),
        timestamp: NOW,
    }
}

// @internal
#[test]
fn own_device_order_sender_failed_commit_does_not_publish_uncommitted_sequence() {
    let pair = Pair::new();
    let mut sender = pair.sender();
    pair.storage.arm_commit_fault();
    assert!(
        sender
            .record_local_change(email("failed@example.test"))
            .is_err()
    );
    assert!(
        pair.storage.connection().is_autocommit(),
        "failed COMMIT must roll back the transaction"
    );
    let target = pair.receiver.identity().unwrap().device_id();
    assert!(
        sender.pending_for_device(target).is_empty(),
        "failed write must not expose an unsaved authenticated sequence"
    );
    sender
        .record_local_change(email("success@example.test"))
        .unwrap();
    let queued = pair.sender();
    assert_eq!(queued.pending_for_device(target).len(), 1);
    let envelopes = queued.build_outbound_envelopes(&pair.sender).unwrap();
    let envelope: SyncEnvelope = serde_json::from_slice(&envelopes[0]).unwrap();
    assert_eq!(pair.apply(&envelope.ciphertext), 1);
    assert_eq!(pair.value("Email"), "success@example.test");
}

// @internal
#[test]
fn own_device_order_cached_senders_do_not_reuse_durable_counter() {
    let pair = Pair::new();
    let mut first = pair.sender();
    let mut second = pair.sender();
    first
        .record_local_change(email("old@example.test"))
        .unwrap();
    let old: SyncEnvelope =
        serde_json::from_slice(&first.build_outbound_envelopes(&pair.sender).unwrap()[0]).unwrap();
    second
        .record_local_change(email("new@example.test"))
        .unwrap();
    let new: SyncEnvelope =
        serde_json::from_slice(&second.build_outbound_envelopes(&pair.sender).unwrap()[0]).unwrap();
    assert_eq!(pair.apply(&old.ciphertext), 1);
    pair.apply(&new.ciphertext);
    assert_eq!(
        pair.value("Email"),
        "new@example.test",
        "cached sender must allocate after the persisted counter"
    );
}

// @internal
#[test]
fn own_device_order_sender_stale_ack_preserves_newly_queued_successor() {
    let pair = Pair::new();
    let mut first = pair.sender();
    first
        .record_local_change(email("old@example.test"))
        .unwrap();
    let mut second = pair.sender();
    second
        .record_local_change(email("new@example.test"))
        .unwrap();
    let target = pair.receiver.identity().unwrap().device_id();
    first.mark_synced(target, 1).unwrap();
    let reloaded = pair.sender();
    assert_eq!(
        reloaded.pending_for_device(target).len(),
        1,
        "old send completion must not erase a later journal write"
    );
    assert!(
        matches!(&reloaded.pending_for_device(target)[0], SyncItem::CardUpdated { new_value, .. } if new_value == "new@example.test")
    );
}

// @internal
#[test]
fn own_device_order_sender_sequence_exhaustion_rejects_without_queuing_or_lock_leak() {
    let pair = Pair::new();
    let key = hex::encode(pair.sender.device_id());
    let vector = crate::sync::VersionVector::from_json(
        &serde_json::json!({"versions": {key: u64::MAX}}).to_string(),
    )
    .unwrap();
    pair.storage.sync().save_version_vector(&vector).unwrap();
    let mut sender = pair.sender();
    assert!(
        sender
            .record_local_change(email("overflow@example.test"))
            .is_err()
    );
    assert!(pair.storage.connection().is_autocommit());
    assert!(pair.sender().devices_with_pending().is_empty());
    assert_eq!(
        pair.sender().version_vector().get(pair.sender.device_id()),
        u64::MAX
    );
}

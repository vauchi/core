// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn own_device_order_failed_apply_does_not_consume_floor_and_is_retryable() {
    let pair = Pair::new();
    let blob = pair.batch("Email", "owner@example.test");
    pair.receiver.storage().connection().execute_batch(
        "CREATE TRIGGER reject_own_card BEFORE INSERT ON own_card BEGIN SELECT RAISE(ABORT, 'injected apply failure'); END;"
    ).unwrap();
    let first = pair
        .receiver
        .apply_device_sync_blob(pair.receiver.identity().unwrap(), &blob);
    pair.receiver
        .storage()
        .connection()
        .execute_batch("DROP TRIGGER reject_own_card;")
        .unwrap();
    let retry = pair
        .receiver
        .apply_device_sync_blob(pair.receiver.identity().unwrap(), &blob)
        .unwrap();
    assert_eq!(
        retry, 1,
        "a failed write must not consume the authenticated sequence"
    );
    assert!(
        first.is_err(),
        "failure must propagate so the live receive loop does not acknowledge lost data"
    );
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert_eq!(pair.apply(&blob), 0);
}

#[test]
fn own_device_order_failed_commit_is_retryable_and_leaves_no_open_transaction() {
    let pair = Pair::new();
    let blob = pair.batch("Email", "owner@example.test");
    pair.receiver.storage().arm_commit_fault();
    let first = pair
        .receiver
        .apply_device_sync_blob(pair.receiver.identity().unwrap(), &blob);
    assert_eq!(pair.apply(&blob), 1);
    assert!(
        first.is_err(),
        "COMMIT failure must propagate to the caller"
    );
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert_eq!(pair.apply(&blob), 0);
}

#[test]
fn own_device_order_removed_contact_note_does_not_block_independent_field() {
    let pair = Pair::new();
    let mut sender = pair.sender();
    sender
        .record_local_change(SyncItem::PersonalNoteChanged {
            contact_id: hex::encode([0x66; 32]),
            note: "delayed note".into(),
            timestamp: NOW,
        })
        .unwrap();
    sender
        .record_local_change(SyncItem::CardUpdated {
            field_label: "Email".into(),
            new_value: "owner@example.test".into(),
            timestamp: NOW,
        })
        .unwrap();
    let envelope: SyncEnvelope =
        serde_json::from_slice(&sender.build_outbound_envelopes(&pair.sender).unwrap()[0]).unwrap();
    assert_eq!(
        pair.apply(&envelope.ciphertext),
        1,
        "intentional note no-op must not roll back an independent field"
    );
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert!(
        pair.receiver
            .get_contact(&hex::encode([0x66; 32]))
            .unwrap()
            .is_none()
    );
    assert_eq!(pair.apply(&envelope.ciphertext), 0);
}

#[test]
fn own_device_order_missing_group_delete_does_not_block_independent_field() {
    let pair = Pair::new();
    let mut sender = pair.sender();
    sender
        .record_local_change(SyncItem::GroupDeleted {
            group_id: "already-removed".into(),
            timestamp: NOW,
        })
        .unwrap();
    sender
        .record_local_change(SyncItem::CardUpdated {
            field_label: "Email".into(),
            new_value: "owner@example.test".into(),
            timestamp: NOW,
        })
        .unwrap();
    let envelope: SyncEnvelope =
        serde_json::from_slice(&sender.build_outbound_envelopes(&pair.sender).unwrap()[0]).unwrap();
    assert_eq!(pair.apply(&envelope.ciphertext), 1);
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert_eq!(pair.apply(&envelope.ciphertext), 0);
}

#[test]
fn own_device_order_initial_floor_read_failure_is_retryable() {
    let pair = Pair::new();
    let blob = pair.batch("Email", "owner@example.test");
    pair.receiver
        .storage()
        .connection()
        .execute_batch("ALTER TABLE device_sync_state RENAME TO unavailable_device_sync_state;")
        .unwrap();
    let first = pair
        .receiver
        .apply_device_sync_blob(pair.receiver.identity().unwrap(), &blob);
    pair.receiver
        .storage()
        .connection()
        .execute_batch("ALTER TABLE unavailable_device_sync_state RENAME TO device_sync_state;")
        .unwrap();
    assert_eq!(pair.apply(&blob), 1);
    assert!(
        first.is_err(),
        "durable floor read failure must not be acknowledged as a dropped malformed message"
    );
    assert_eq!(pair.value("Email"), "owner@example.test");
}

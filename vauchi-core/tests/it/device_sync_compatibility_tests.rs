// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mixed-version tolerance for owner-sync item decoding (Release A of
//! the readers-before-writers rollout,
//! `backlog/2026-07-21-per-device-ratchet-registry-dormant` §Progress 3).
//!
//! A linked device running a newer core may emit `SyncItem` variants this
//! binary does not know. Batch-wide `Vec<SyncItem>` decoding rejects the
//! whole batch on the first unknown variant, silently dropping every known
//! sibling item with it. These tests pin the tolerant contract: unknown
//! variants are counted and skipped, known items in the same batch survive,
//! and malformed known items are isolated rather than fatal.

use vauchi_core::sync::{
    DecodedSyncItems, InterDeviceSyncState, SyncItem, decode_sync_items_tolerantly,
};

fn known_contact_removed_json(id: &str, ts: u64) -> String {
    format!(r#"{{"ContactRemoved":{{"contact_id":"{id}","timestamp":{ts}}}}}"#)
}

// @scenario: sync_updates :: Linked devices stay in sync across app versions
// @internal
#[test]
fn unknown_sync_item_does_not_drop_known_items_in_same_batch() {
    let batch = format!(
        "[{},{},{}]",
        known_contact_removed_json("contact-a", 1000),
        r#"{"FutureVariantFromNewerVersion":{"anything":"goes","nested":{"x":1}}}"#,
        known_contact_removed_json("contact-b", 2000),
    );

    let decoded: DecodedSyncItems =
        decode_sync_items_tolerantly(batch.as_bytes()).expect("a batch with unknowns must decode");

    assert_eq!(
        decoded.known.len(),
        2,
        "both known items must survive the unknown sibling"
    );
    assert_eq!(decoded.unknown_count, 1, "the unknown variant is counted");
    assert_eq!(decoded.malformed_count, 0);
    let timestamps: Vec<u64> = decoded.known.iter().map(|i| i.timestamp()).collect();
    assert_eq!(
        timestamps,
        vec![1000, 2000],
        "known items keep their original order"
    );
}

// @scenario: sync_updates :: Linked devices stay in sync across app versions
// @internal
#[test]
fn malformed_known_sync_item_is_isolated_not_fatal() {
    let batch = format!(
        "[{},{},{}]",
        known_contact_removed_json("contact-a", 1000),
        // Known variant name, wrong field type — must not kill the batch.
        r#"{"ContactRemoved":{"contact_id":42,"timestamp":"not-a-number"}}"#,
        known_contact_removed_json("contact-b", 2000),
    );

    let decoded =
        decode_sync_items_tolerantly(batch.as_bytes()).expect("malformed items must be isolated");

    assert_eq!(decoded.known.len(), 2);
    assert_eq!(decoded.unknown_count, 0);
    assert_eq!(decoded.malformed_count, 1, "the malformed item is counted");
}

// @scenario: sync_updates :: Linked devices stay in sync across app versions
// @internal
#[test]
fn non_array_and_oversized_batches_fail_closed() {
    assert!(
        decode_sync_items_tolerantly(br#"{"ContactRemoved":{}}"#).is_err(),
        "a non-array batch is rejected"
    );
    assert!(
        decode_sync_items_tolerantly(b"not json").is_err(),
        "unparsable bytes are rejected"
    );

    let one = known_contact_removed_json("c", 1);
    let huge = format!("[{}]", vec![one; 10_001].join(","));
    assert!(
        decode_sync_items_tolerantly(huge.as_bytes()).is_err(),
        "a batch above the item-count bound is rejected (DC-01)"
    );
}

// @scenario: sync_updates :: Linked devices stay in sync across app versions
// @internal
#[test]
fn unknown_pending_sync_item_does_not_block_state_restore() {
    // Simulates InterDeviceSyncState persisted by a newer build whose
    // pending queue holds a variant this binary does not know: splice an
    // unknown item into a genuinely serialized state.
    let mut state = InterDeviceSyncState::new([7u8; 32]);
    state.queue_item(SyncItem::ContactRemoved {
        contact_id: "contact-a".into(),
        timestamp: 1000,
    });
    state.mark_synced(9);
    state.queue_item(SyncItem::ContactRemoved {
        contact_id: "contact-a".into(),
        timestamp: 1000,
    });
    let state_json = state.to_json().replace(
        r#"],"last_sync_version""#,
        r#",{"FutureVariantFromNewerVersion":{"v":2}}],"last_sync_version""#,
    );
    assert!(
        state_json.contains("FutureVariantFromNewerVersion"),
        "splice premise: the serialized layout matched"
    );

    let restored = InterDeviceSyncState::from_json(&state_json)
        .expect("an unknown pending item must not block state restore");

    assert_eq!(restored.device_id(), &[7u8; 32]);
    assert_eq!(restored.last_sync_version(), 9);
    assert_eq!(
        restored.pending_items().len(),
        1,
        "the known pending item survives; the unknown one is skipped"
    );
    assert_eq!(restored.pending_items()[0].timestamp(), 1000);
}

// @scenario: sync_updates :: Linked devices stay in sync across app versions
// @internal
#[test]
fn state_roundtrip_of_known_items_is_unchanged() {
    // Control: the tolerant reader must not alter the strict happy path.
    let mut state = InterDeviceSyncState::new([9u8; 32]);
    state.queue_item(SyncItem::ContactRemoved {
        contact_id: "contact-x".into(),
        timestamp: 4242,
    });

    let restored = InterDeviceSyncState::from_json(&state.to_json()).unwrap();

    assert_eq!(restored.device_id(), &[9u8; 32]);
    assert_eq!(restored.pending_items().len(), 1);
    assert_eq!(restored.pending_items()[0].timestamp(), 4242);
}

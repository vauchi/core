// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn own_device_order_released_reader_skips_metadata_and_keeps_body() {
    let pair = Pair::new();
    let blob = pair.batch("Email", "owner@example.test");
    let decoded = crate::sync::decode_sync_items_tolerantly(&pair.plaintext(&blob)).unwrap();
    assert_eq!(decoded.known.len(), 1);
    assert_eq!(decoded.unknown_count, 1);
    assert_eq!(decoded.malformed_count, 0);
    assert!(
        matches!(&decoded.known[0], SyncItem::CardUpdated { new_value, .. } if new_value == "owner@example.test")
    );
}

#[test]
fn own_device_order_rejects_bad_metadata_without_legacy_fallback() {
    use serde_json::json;
    let pair = Pair::new();
    let blob = pair.batch("Email", "owner@example.test");
    let valid: serde_json::Value = serde_json::from_slice(&pair.plaintext(&blob)).unwrap();
    let mut invalid = Vec::new();
    for (field, value) in [
        ("version", json!(2)),
        ("sender", json!(vec![0; 32])),
        ("recipient", json!(vec![0; 32])),
        ("sequences", json!([])),
        ("sequences", json!([0])),
        ("sequences", json!([18446744073709551616.0f64])),
    ] {
        let mut value_batch = valid.clone();
        value_batch[0]["_VauchiDeviceSyncOrder"][field] = value;
        invalid.push(value_batch);
    }
    let mut duplicate = valid.clone();
    duplicate.as_array_mut().unwrap().push(valid[0].clone());
    invalid.push(duplicate);
    let mut misplaced = valid.clone();
    misplaced.as_array_mut().unwrap().swap(0, 1);
    invalid.push(misplaced);
    let mut shape = valid.clone();
    shape[0]["extra"] = json!(true);
    invalid.push(shape);
    for malformed in invalid {
        assert_eq!(
            pair.apply(&pair.seal(&malformed)),
            0,
            "bad metadata must not apply its otherwise valid body: {malformed}"
        );
    }
    assert_eq!(
        pair.apply(&blob),
        1,
        "rejected metadata must not poison subsequent valid delivery"
    );
}

#[test]
fn own_device_order_unknown_siblings_keep_sequence_positions_and_prevent_downgrade() {
    use serde_json::json;
    let pair = Pair::new();
    let old = pair.batch("Email", "old@example.test");
    let new = pair.batch("Email", "new@example.test");
    let old_json: serde_json::Value = serde_json::from_slice(&pair.plaintext(&old)).unwrap();
    let mut batch: serde_json::Value = serde_json::from_slice(&pair.plaintext(&new)).unwrap();
    batch[0]["_VauchiDeviceSyncOrder"]["sequences"] = json!([1, 2, 3]);
    batch.as_array_mut().unwrap().insert(1, old_json[1].clone());
    batch
        .as_array_mut()
        .unwrap()
        .insert(2, json!({"FutureVariant":{"timestamp": NOW}}));
    assert_eq!(pair.apply(&pair.seal(&batch)), 2);
    assert_eq!(pair.value("Email"), "new@example.test");
    assert_eq!(pair.apply(&old), 0);
    assert_eq!(pair.apply(&pair.seal(&json!([old_json[1].clone()]))), 0);
}

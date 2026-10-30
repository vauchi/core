// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::sync::InterDeviceSyncState;

// @internal
#[test]
fn own_device_order_state_serde_legacy_resume_preserves_existing_pending_item() {
    let mut old = InterDeviceSyncState::new([7; 32]);
    old.queue_item(SyncItem::CardUpdated {
        field_label: "Email".into(),
        new_value: "old@example.test".into(),
        timestamp: NOW,
    });
    let mut restored: InterDeviceSyncState = serde_json::from_str(&old.to_json()).unwrap();
    restored.queue_item(SyncItem::CardUpdated {
        field_label: "Phone".into(),
        new_value: "+41000000000".into(),
        timestamp: NOW,
    });
    let again = InterDeviceSyncState::from_json(&restored.to_json()).unwrap();
    assert_eq!(
        again.pending_items().len(),
        2,
        "Serde legacy restore must align existing item positions before appending"
    );
}

// @internal
#[test]
fn own_device_order_state_tolerant_restore_zips_before_skipping_unknown_items() {
    let mut old = InterDeviceSyncState::new([7; 32]);
    old.queue_item(SyncItem::CardUpdated {
        field_label: "Email".into(),
        new_value: "old@example.test".into(),
        timestamp: NOW,
    });
    let mut raw: serde_json::Value = serde_json::from_str(&old.to_json()).unwrap();
    raw["pending_items"]
        .as_array_mut()
        .unwrap()
        .insert(0, serde_json::json!({"FutureVariant": {}}));
    raw["pending_sequences"] = serde_json::json!([1, 2]);
    let restored = InterDeviceSyncState::from_json(&raw.to_string()).unwrap();
    let serialized: serde_json::Value = serde_json::from_str(&restored.to_json()).unwrap();
    assert_eq!(serialized["pending_sequences"], serde_json::json!([2]));
    raw["pending_sequences"] = serde_json::json!([1]);
    assert!(InterDeviceSyncState::from_json(&raw.to_string()).is_err());
}

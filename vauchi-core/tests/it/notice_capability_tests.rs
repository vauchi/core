// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device's cohort support is authenticated, versioned and mergeable.

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, Reconciled, RegistryBroadcast};

#[path = "notice_capability_acceptance_tests.rs"]
mod acceptance;

fn registry() -> (DeviceRegistry, SigningKeyPair, [u8; 32]) {
    let key = SigningKeyPair::generate();
    let device = DeviceInfo::derive(&[7; 32], 0, "Phone".into(), 0).to_registered(&[7; 32]);
    let id = device.device_id;
    (DeviceRegistry::new(device, &key), key, id)
}

fn merge(a: &DeviceRegistry, b: &DeviceRegistry, key: &SigningKeyPair) -> DeviceRegistry {
    match a.reconcile(b, key).unwrap() {
        Reconciled::Keep => a.clone(),
        Reconciled::Adopt => b.clone(),
        Reconciled::Publish(merged) => merged,
    }
}

// @internal
#[test]
fn a_cohort_announcement_is_signed_and_repeated_configuration_is_a_noop() {
    let (mut registry, key, id) = registry();
    let initial = registry.version();
    assert!(registry.set_notice_cohort(&id, Some(7), &key).unwrap());
    let version = registry.version();
    assert!(version > initial);
    assert!(!registry.set_notice_cohort(&id, Some(7), &key).unwrap());
    assert_eq!(registry.version(), version);
    let broadcast = RegistryBroadcast::new(&registry, &key, 1);
    assert_eq!(broadcast.active_devices()[0].notice_cohort, Some(7));
    assert!(broadcast.verify(&key.public_key()));
    let mut changed: serde_json::Value = serde_json::from_str(&broadcast.to_json()).unwrap();
    changed["active_devices"][0]["notice_cohort"] = serde_json::json!(8);
    let changed = RegistryBroadcast::from_json(&serde_json::to_string(&changed).unwrap()).unwrap();
    assert!(!changed.verify(&key.public_key()));
}

// @internal
#[test]
fn disabling_support_survives_a_stale_announcement_in_either_merge_order() {
    let (mut current, key, id) = registry();
    current.set_notice_cohort(&id, Some(7), &key).unwrap();
    let stale = current.clone();
    current.set_notice_cohort(&id, None, &key).unwrap();
    let left = merge(&current, &stale, &key);
    let right = merge(&stale, &current, &key);
    for merged in [left, right] {
        let device = merged.find_device(&id).unwrap();
        assert_eq!(device.notice_cohort, None);
        assert_eq!(device.notice_capability_version, 2);
        assert!(merged.verify(&key.public_key()));
    }
}

// @internal
#[test]
fn removing_the_capability_tombstone_invalidates_the_broadcast_signature() {
    let (mut registry, key, id) = registry();
    registry.set_notice_cohort(&id, Some(7), &key).unwrap();
    registry.set_notice_cohort(&id, None, &key).unwrap();
    let broadcast = RegistryBroadcast::new(&registry, &key, 1);
    let mut changed: serde_json::Value = serde_json::from_str(&broadcast.to_json()).unwrap();
    changed["active_devices"][0]
        .as_object_mut()
        .unwrap()
        .remove("notice_capability_version");
    let changed = RegistryBroadcast::from_json(&serde_json::to_string(&changed).unwrap()).unwrap();
    assert!(!changed.verify(&key.public_key()));
}

// @internal
#[test]
fn a_registry_that_never_announced_cohorts_keeps_its_existing_wire_shape() {
    let (registry, key, _) = registry();
    let text = RegistryBroadcast::new(&registry, &key, 1).to_json();
    assert!(!text.contains("notice_cohort"));
    assert!(!text.contains("notice_capability_version"));
    assert!(
        RegistryBroadcast::from_json(&text)
            .unwrap()
            .verify(&key.public_key())
    );
}

proptest::proptest! {
    // @internal
    #[test]
    fn any_stale_capability_snapshot_keeps_the_latest_withdrawal_or_announcement(
        changes in proptest::collection::vec(proptest::option::of(0u32..10), 1..40),
        cut in 0usize..100,
    ) {
        let (mut current, key, id) = registry();
        let mut history = vec![current.clone()];
        let mut expected = None;
        let mut revision = 0;
        for change in &changes {
            if *change != expected { revision += 1; }
            current.set_notice_cohort(&id, *change, &key).unwrap();
            expected = *change;
            history.push(current.clone());
        }
        let stale = &history[cut % history.len()];
        for merged in [merge(&current, stale, &key), merge(stale, &current, &key)] {
            let device = merged.find_device(&id).unwrap();
            proptest::prop_assert_eq!(device.notice_cohort, expected);
            proptest::prop_assert_eq!(device.notice_capability_version, revision);
            proptest::prop_assert!(merged.verify(&key.public_key()));
        }
    }
}

// @internal
#[test]
fn conflicting_capability_announcements_merge_the_same_in_both_orders() {
    let (mut left, key, id) = registry();
    let mut right = left.clone();
    left.set_notice_cohort(&id, Some(7), &key).unwrap();
    right.set_notice_cohort(&id, Some(8), &key).unwrap();
    let a = merge(&left, &right, &key);
    let b = merge(&right, &left, &key);
    assert_eq!(a.find_device(&id).unwrap().notice_cohort, Some(7));
    assert_eq!(b.find_device(&id).unwrap().notice_cohort, Some(7));
    // A side that already holds the winning value may keep its version;
    // one more exchange adopts the other side's published merge version.
    assert_eq!(merge(&a, &b, &key).to_json(), merge(&b, &a, &key).to_json());
}

// @internal
#[test]
fn an_exhausted_capability_revision_cannot_rewind_or_change_the_announcement() {
    let (registry, key, id) = registry();
    let mut device = registry.find_device(&id).unwrap().clone();
    device.notice_cohort = Some(7);
    device.notice_capability_version = u64::MAX;
    let mut registry = DeviceRegistry::new(device, &key);
    let before = registry.to_json();
    let failed = registry.set_notice_cohort(&id, None, &key);
    assert!(matches!(
        failed,
        Err(vauchi_core::identity::device::DeviceError::CapabilityVersionExhausted)
    ));
    assert_eq!(registry.to_json(), before);
}

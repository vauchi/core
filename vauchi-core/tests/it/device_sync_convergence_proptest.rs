// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 stateful property test: two-device sync converges (G5 / Phase 4).
//!
//! Models two same-identity devices, each with its own storage and
//! `DeviceSyncOrchestrator`, under random interleavings of local edits
//! and bidirectional sync. Invariant: after a full quiescent sync, both
//! devices resolve every field to the **latest write** (last-write-wins,
//! ADR-020) — no lost update, no divergence.
//!
//! Timestamps model real per-device logical clocks: a monotonically
//! increasing global counter assigns a unique stamp to each edit, so the
//! latest write is always well-defined. (Exact-timestamp ties across
//! devices — where ADR-020's device-id tie-break would apply — are out of
//! scope here: `SyncItem` does not yet carry the originating device id, so
//! the tie-break is unimplemented. Tracked as a follow-up; see
//! `2026-06-06-multi-device-sync-live-wiring`.)
//!
//! @scenario: device_management.feature - Concurrent edits converge by last-write-wins

use std::collections::HashMap;

use proptest::prelude::*;
use vauchi_core::Storage;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::SyncItem;

const SEED: [u8; 32] = [0x7c; 32];
const KEYS: u8 = 4; // distinct fields edited

fn device(index: u32) -> DeviceInfo {
    DeviceInfo::derive(&SEED, index, format!("device-{index}"), 0)
}

fn two_device_registry() -> DeviceRegistry {
    let signing = SigningKeyPair::from_seed(&SEED);
    let mut reg = DeviceRegistry::new(device(0).to_registered(&SEED), &signing);
    reg.add_device_unsigned(device(1).to_registered(&SEED))
        .unwrap();
    reg
}

fn field_label(key: u8) -> String {
    format!("f{key}")
}

#[derive(Debug, Clone)]
enum Op {
    /// Edit `key` on device A (true) or B (false).
    Edit { on_a: bool, key: u8 },
    /// One bidirectional sync exchange.
    Sync,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<bool>(), 0..KEYS).prop_map(|(on_a, key)| Op::Edit { on_a, key }),
        Just(Op::Sync),
    ]
}

/// Drives one device's pending queue into the other and records the
/// timestamps the receiver accepts into `recv_resolved`.
fn push(
    sender: &mut DeviceSyncOrchestrator<'_>,
    sender_id: &[u8; 32],
    receiver: &mut DeviceSyncOrchestrator<'_>,
    target_id: &[u8; 32],
    recv_resolved: &mut HashMap<String, u64>,
) {
    let msg = sender.create_sync_message(target_id).unwrap();
    if msg.items.is_empty() {
        return;
    }
    let version = sender.version_vector().get(sender_id);
    let applied = receiver.process_incoming(msg.items).unwrap();
    for item in &applied {
        if let SyncItem::CardUpdated {
            field_label,
            timestamp,
            ..
        } = item
        {
            recv_resolved.insert(field_label.clone(), *timestamp);
        }
    }
    sender.mark_synced(target_id, version).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]

    // @scenario: device_management :: Concurrent edits converge by last-write-wins
    #[test]
    fn two_devices_converge_to_latest_write(ops in prop::collection::vec(op_strategy(), 1..40)) {
        let storage_a = Storage::in_memory(SymmetricKey::generate()).unwrap();
        let storage_b = Storage::in_memory(SymmetricKey::generate()).unwrap();
        let registry = two_device_registry();

        let mut orch_a = DeviceSyncOrchestrator::new(&storage_a, device(0), registry.clone());
        let mut orch_b = DeviceSyncOrchestrator::new(&storage_b, device(1), registry.clone());

        let id_a = *device(0).device_id();
        let id_b = *device(1).device_id();

        // Per-device view of the latest accepted timestamp per field.
        let mut resolved_a: HashMap<String, u64> = HashMap::new();
        let mut resolved_b: HashMap<String, u64> = HashMap::new();
        // Independent oracle: the globally-latest write per field.
        let mut oracle: HashMap<String, u64> = HashMap::new();

        let mut clock: u64 = 1; // unique, increasing logical timestamps

        for op in ops {
            match op {
                Op::Edit { on_a, key } => {
                    let label = field_label(key);
                    let ts = clock;
                    clock += 1;
                    let item = SyncItem::CardUpdated {
                        field_label: label.clone(),
                        new_value: ts.to_string(),
                        timestamp: ts,
                    };
                    if on_a {
                        orch_a.record_local_change(item).unwrap();
                        resolved_a.insert(label.clone(), ts);
                    } else {
                        orch_b.record_local_change(item).unwrap();
                        resolved_b.insert(label.clone(), ts);
                    }
                    oracle.insert(label, ts); // monotonic clock => always newest
                }
                Op::Sync => {
                    push(&mut orch_a, &id_a, &mut orch_b, &id_b, &mut resolved_b);
                    push(&mut orch_b, &id_b, &mut orch_a, &id_a, &mut resolved_a);
                }
            }
        }

        // Final quiescent drain — bounded; each round must make progress.
        for _ in 0..(KEYS as usize + 4) {
            if orch_a.devices_with_pending().is_empty()
                && orch_b.devices_with_pending().is_empty()
            {
                break;
            }
            push(&mut orch_a, &id_a, &mut orch_b, &id_b, &mut resolved_b);
            push(&mut orch_b, &id_b, &mut orch_a, &id_a, &mut resolved_a);
        }

        // Convergence: both devices reached the latest-write oracle.
        prop_assert!(
            orch_a.devices_with_pending().is_empty(),
            "device A still has unsynced items after drain"
        );
        prop_assert!(
            orch_b.devices_with_pending().is_empty(),
            "device B still has unsynced items after drain"
        );
        prop_assert_eq!(&resolved_a, &oracle, "device A diverged from latest-write");
        prop_assert_eq!(&resolved_b, &oracle, "device B diverged from latest-write");
    }
}

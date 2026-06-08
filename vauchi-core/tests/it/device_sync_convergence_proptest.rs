// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 stateful property test: two-device sync converges (G5 / Phase 4).
//!
//! Models two same-identity devices, each with its own storage and
//! `DeviceSyncOrchestrator`, under random interleavings of local edits
//! (including **concurrent same-timestamp edits**) and bidirectional sync.
//! Invariant: after a full quiescent sync, both devices resolve every
//! field to the same winner — the lexicographically-greatest
//! `(timestamp, device_id)` write (ADR-020 LWW + device-id tie-break) —
//! so there is no lost update and no divergence, even on exact ties.
//!
//! @scenario: device_management.feature - Concurrent edits converge by last-write-wins

use std::collections::HashMap;

use proptest::prelude::*;
use vauchi_core::Storage;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::{FieldStamp, SyncItem};

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

/// Record the winner-so-far for a field: keep the entry with the greater
/// `(timestamp, device_id)` stamp. Mirrors `process_incoming`'s rule.
fn observe(
    map: &mut HashMap<String, (FieldStamp, String)>,
    key: String,
    stamp: FieldStamp,
    value: String,
) {
    match map.get(&key) {
        Some((existing, _)) if *existing >= stamp => {}
        _ => {
            map.insert(key, (stamp, value));
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    /// One device edits `key` (unique timestamp).
    Edit { on_a: bool, key: u8 },
    /// Both devices edit `key` at the SAME timestamp — a tie that must
    /// resolve by device id.
    Concurrent { key: u8 },
    /// One bidirectional sync exchange.
    Sync,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<bool>(), 0..KEYS).prop_map(|(on_a, key)| Op::Edit { on_a, key }),
        1 => (0..KEYS).prop_map(|key| Op::Concurrent { key }),
        3 => Just(Op::Sync),
    ]
}

/// Pushes one device's pending queue into the other, recording the
/// timestamps+values the receiver accepts into `recv_resolved` (stamped
/// with the sending device's id — the ADR-020 tie-breaker).
fn push(
    sender: &mut DeviceSyncOrchestrator<'_>,
    sender_id: &[u8; 32],
    receiver: &mut DeviceSyncOrchestrator<'_>,
    target_id: &[u8; 32],
    recv_resolved: &mut HashMap<String, (FieldStamp, String)>,
) {
    let msg = sender.create_sync_message(target_id).unwrap();
    if msg.items.is_empty() {
        return;
    }
    let version = sender.version_vector().get(sender_id);
    let applied = receiver.process_incoming(msg.items, sender_id).unwrap();
    for item in &applied {
        if let SyncItem::CardUpdated {
            field_label,
            new_value,
            timestamp,
        } = item
        {
            observe(
                recv_resolved,
                field_label.clone(),
                FieldStamp {
                    timestamp: *timestamp,
                    device_id: *sender_id,
                },
                new_value.clone(),
            );
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

        // Per-device view + independent oracle: winning (stamp, value) per field.
        let mut resolved_a: HashMap<String, (FieldStamp, String)> = HashMap::new();
        let mut resolved_b: HashMap<String, (FieldStamp, String)> = HashMap::new();
        let mut oracle: HashMap<String, (FieldStamp, String)> = HashMap::new();

        let mut clock: u64 = 1; // increasing logical timestamps (per-device monotonic)

        for op in ops {
            match op {
                Op::Edit { on_a, key } => {
                    let label = field_label(key);
                    let ts = clock;
                    clock += 1;
                    let (orch, id, who) = if on_a {
                        (&mut orch_a, id_a, "A")
                    } else {
                        (&mut orch_b, id_b, "B")
                    };
                    let value = format!("{ts}-{who}");
                    orch.record_local_change(SyncItem::CardUpdated {
                        field_label: label.clone(),
                        new_value: value.clone(),
                        timestamp: ts,
                    })
                    .unwrap();
                    let stamp = FieldStamp { timestamp: ts, device_id: id };
                    observe(
                        if on_a { &mut resolved_a } else { &mut resolved_b },
                        label.clone(),
                        stamp,
                        value.clone(),
                    );
                    observe(&mut oracle, label, stamp, value);
                }
                Op::Concurrent { key } => {
                    let label = field_label(key);
                    let ts = clock;
                    clock += 1;
                    let val_a = format!("{ts}-A-concurrent");
                    let val_b = format!("{ts}-B-concurrent");
                    orch_a
                        .record_local_change(SyncItem::CardUpdated {
                            field_label: label.clone(),
                            new_value: val_a.clone(),
                            timestamp: ts,
                        })
                        .unwrap();
                    orch_b
                        .record_local_change(SyncItem::CardUpdated {
                            field_label: label.clone(),
                            new_value: val_b.clone(),
                            timestamp: ts,
                        })
                        .unwrap();
                    let stamp_a = FieldStamp { timestamp: ts, device_id: id_a };
                    let stamp_b = FieldStamp { timestamp: ts, device_id: id_b };
                    observe(&mut resolved_a, label.clone(), stamp_a, val_a.clone());
                    observe(&mut resolved_b, label.clone(), stamp_b, val_b.clone());
                    observe(&mut oracle, label.clone(), stamp_a, val_a);
                    observe(&mut oracle, label, stamp_b, val_b);
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

        prop_assert!(
            orch_a.devices_with_pending().is_empty(),
            "device A still has unsynced items after drain"
        );
        prop_assert!(
            orch_b.devices_with_pending().is_empty(),
            "device B still has unsynced items after drain"
        );
        prop_assert_eq!(&resolved_a, &oracle, "device A diverged from the LWW+device-id winner");
        prop_assert_eq!(&resolved_b, &oracle, "device B diverged from the LWW+device-id winner");
    }
}

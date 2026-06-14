// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 stateful property tests for the TransportReadiness ledger
//! (`2026-06-11-exchange-waits-forever-without-capabilities` Phase 2).
//!
//! The ledger is a small state machine over per-requirement permission state.
//! These properties pin its invariants across random operation sequences:
//!
//! - **Last-write-wins.** `permission(req)` always reflects the most recent
//!   `note_denied`/`note_granted` for `req` (or `Unknown` if none) — a grant
//!   recovers from a prior denial and vice-versa.
//! - **Presence × permission combine.** `requirement_readiness` is consistent
//!   with both dimensions: `HardwareAbsent` only when the hardware is absent;
//!   `PermissionDenied` only when present and denied; `Ready` only when present
//!   and not denied. Hardware absence dominates a denial (no grant path).

use proptest::prelude::*;
use std::collections::HashMap;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::capability::{
    PermissionState, RequirementReadiness, TransportReadiness,
};
use vauchi_core::exchange::mode::DeviceRequirement;
use vauchi_core::types::AudioCapability;

/// Every requirement the ledger can key on. Kept in sync with
/// `DeviceRequirement` by the exhaustive `oracle_present` match below.
const ALL_REQS: [DeviceRequirement; 8] = [
    DeviceRequirement::Camera,
    DeviceRequirement::Ble,
    DeviceRequirement::Nfc,
    DeviceRequirement::Microphone,
    DeviceRequirement::Speaker,
    DeviceRequirement::Accelerometer,
    DeviceRequirement::Internet,
    DeviceRequirement::UsbPort,
];

fn requirement_strategy() -> impl Strategy<Value = DeviceRequirement> {
    prop_oneof![
        Just(DeviceRequirement::Camera),
        Just(DeviceRequirement::Ble),
        Just(DeviceRequirement::Nfc),
        Just(DeviceRequirement::Microphone),
        Just(DeviceRequirement::Speaker),
        Just(DeviceRequirement::Accelerometer),
        Just(DeviceRequirement::Internet),
        Just(DeviceRequirement::UsbPort),
    ]
}

/// One ledger op: `(is_grant, req)` — grant when true, deny when false.
fn op_strategy() -> impl Strategy<Value = (bool, DeviceRequirement)> {
    (any::<bool>(), requirement_strategy())
}

fn caps_strategy() -> impl Strategy<Value = DeviceCapabilities> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        0u8..4,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(cam, ble, nfc, audio_n, accel, net, usb)| DeviceCapabilities {
                has_camera: cam,
                has_ble: ble,
                has_nfc: nfc,
                audio: match audio_n {
                    0 => AudioCapability::None,
                    1 => AudioCapability::Full,
                    2 => AudioCapability::EmitOnly,
                    _ => AudioCapability::ReceiveOnly,
                },
                has_accelerometer: accel,
                has_internet: net,
                has_usb_port: usb,
                ..Default::default()
            },
        )
}

/// Independent oracle for hardware presence — deliberately re-derived here so a
/// drift in production's `requirement_present` join map is caught (not a call
/// into the code under test).
fn oracle_present(req: DeviceRequirement, caps: &DeviceCapabilities) -> bool {
    match req {
        DeviceRequirement::Camera => caps.has_camera,
        DeviceRequirement::Ble => caps.has_ble,
        DeviceRequirement::Nfc => caps.has_nfc,
        DeviceRequirement::Microphone => {
            matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::ReceiveOnly
            )
        }
        DeviceRequirement::Speaker => {
            matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::EmitOnly
            )
        }
        DeviceRequirement::Accelerometer => caps.has_accelerometer,
        DeviceRequirement::Internet => caps.has_internet,
        DeviceRequirement::UsbPort => caps.has_usb_port,
        // `DeviceRequirement` is #[non_exhaustive], so this wildcard is
        // mandatory for an out-of-crate match. It is unreachable for the
        // strategy's known variants; a new variant must be added to
        // `ALL_REQS`, `requirement_strategy`, and the arms above together
        // (no compile-time parity is possible across the crate boundary).
        other => {
            unreachable!("oracle missing arm for {other:?} — keep in sync with DeviceRequirement")
        }
    }
}

fn apply(led: &mut TransportReadiness, ops: &[(bool, DeviceRequirement)]) {
    for (grant, req) in ops {
        if *grant {
            led.note_granted(*req);
        } else {
            led.note_denied(*req);
        }
    }
}

proptest! {
    /// Last-write-wins: `permission(req)` equals the last op applied to `req`.
    // @internal
    #[test]
    fn permission_is_last_write_wins(ops in prop::collection::vec(op_strategy(), 0..40)) {
        let mut led = TransportReadiness::new();
        let mut expected: HashMap<DeviceRequirement, PermissionState> = HashMap::new();
        for (grant, req) in &ops {
            apply(&mut led, std::slice::from_ref(&(*grant, *req)));
            expected.insert(
                *req,
                if *grant { PermissionState::Granted } else { PermissionState::Denied },
            );
        }
        for req in ALL_REQS {
            let want = expected.get(&req).copied().unwrap_or(PermissionState::Unknown);
            prop_assert_eq!(led.permission(req), want, "req {:?}", req);
        }
    }

    /// `requirement_readiness` is consistent with presence and permission.
    // @internal
    #[test]
    fn readiness_combines_presence_and_permission(
        ops in prop::collection::vec(op_strategy(), 0..40),
        caps in caps_strategy(),
    ) {
        let mut led = TransportReadiness::new();
        apply(&mut led, &ops);
        for req in ALL_REQS {
            let present = oracle_present(req, &caps);
            match led.requirement_readiness(req, &caps) {
                RequirementReadiness::HardwareAbsent => {
                    prop_assert!(!present, "HardwareAbsent but {:?} is present", req);
                }
                RequirementReadiness::PermissionDenied => {
                    prop_assert!(present, "PermissionDenied but {:?} absent", req);
                    prop_assert_eq!(led.permission(req), PermissionState::Denied);
                }
                RequirementReadiness::Ready => {
                    prop_assert!(present, "Ready but {:?} absent", req);
                    prop_assert_ne!(led.permission(req), PermissionState::Denied);
                }
            }
        }
    }
}

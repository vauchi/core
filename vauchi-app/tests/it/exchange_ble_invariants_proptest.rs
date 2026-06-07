// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based invariants for the dedicated `BleExchangeEngine`
//! (CC-13: stateful proptest for state machines).
//!
//! Phase 1.2 of `_private/docs/problems/2026-05-03-ble-exchange-humble-ui-migration`.
//! The BLE flow plugs into `ExchangeEngine` as a sub-flow (Phase 0
//! lock-in). Per-event unit tests already cover individual
//! transitions; this file pins the cross-sequence invariant: once
//! the engine reaches `ExchangeStep::Success` via the BLE happy
//! path, no random hardware-event sequence can transition it away.
//!
//! Invariant: **`Success` is terminal.** A flow that has completed
//! exchange must not silently re-open or leak into another step
//! when stray BLE / proximity / hardware-error events arrive.
//! Symptom this catches in the wild would be R-1 silent state
//! desync (see record's risk register) — e.g. a late
//! `BleDisconnected` after Complete corrupting the success state
//! and dropping the saved card.

use proptest::prelude::*;
use vauchi_app::ui::{BleExchangeEngine, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::exchange::mode::ExchangeMode;

/// Strategy: arbitrary BLE / proximity / hardware-error events that
/// could plausibly arrive after exchange completes — spurious scans,
/// stale GATT notifications, late disconnects, transport errors.
fn arb_post_complete_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        // BLE radio events
        Just(Event::BleDeviceDiscovered {
            id: "spurious".into(),
            rssi: -50,
            adv_data: vec![],
        }),
        Just(Event::BleConnected {
            device_id: "spurious".into(),
        }),
        Just(Event::BleDisconnected {
            reason: "late disconnect".into(),
        }),
        Just(Event::BleCharacteristicNotified {
            uuid: "spurious".into(),
            data: vec![1, 2, 3],
        }),
        Just(Event::BleCharacteristicRead {
            uuid: "spurious".into(),
            data: vec![],
        }),
        // Proximity events (could fire from a still-recording adapter)
        Just(Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3500,
        }),
        Just(Event::AccelerometerData {
            x_milli_g: 100,
            y_milli_g: 0,
            z_milli_g: 0,
            timestamp_ms: 0,
        }),
        // Transport-level error events scoped to BLE
        Just(Event::HardwareError {
            transport: "ble".into(),
            error: "spurious".into(),
        }),
        Just(Event::PermissionDenied {
            transport: "ble".into(),
        }),
        Just(Event::HardwareUnavailable {
            transport: "ble".into(),
        }),
    ]
}

/// Drive `BleExchangeEngine` through the Magic-mode happy path to its
/// terminal `Success` screen. Post BLE graduation the BLE flow lives in
/// the dedicated engine, not the legacy `ExchangeEngine` sub-flow.
fn drive_magic_to_success() -> BleExchangeEngine {
    let mut e = BleExchangeEngine::new(ExchangeMode::Magic, true, vec![]);
    e.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    e.handle_hardware_event(Event::BleConnected {
        device_id: "peer-1".into(),
    });
    // P4: the hollow flow no longer self-completes from notified bytes;
    // the real `BleHandshakeMachine` completion drives the chrome to
    // Success via `force_success`. Call it to reach the terminal screen
    // (the invariant under test is that Success stays terminal).
    e.force_success();
    assert_eq!(e.current_screen().screen_id, "exchange_success");
    e
}

proptest! {
    /// Once the engine reaches `Success` via the BLE happy path,
    /// no random sequence of post-exchange hardware events drops
    /// it back into a non-terminal screen. The user-visible
    /// invariant: the `Done` button stays available; the saved
    /// card stays saved.
    // @internal
    #[test]
    fn success_step_is_terminal_under_random_post_complete_events(
        events in proptest::collection::vec(arb_post_complete_event(), 0..32),
    ) {
        let mut engine = drive_magic_to_success();
        for event in events {
            engine.handle_hardware_event(event);
            // Success screen must remain reachable. We check via
            // screen_id rather than the private `step` field —
            // matches the user-facing invariant the harness needs
            // to enforce.
            prop_assert_eq!(
                engine.current_screen().screen_id,
                "exchange_success",
                "Success screen replaced after random post-complete events",
            );
        }
    }
}

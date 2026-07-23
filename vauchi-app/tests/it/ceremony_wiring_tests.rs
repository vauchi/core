// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S4 — ceremony Phase 0 (design D2.4,
//! `2026-07-03-exchange-ceremony-unshipped`,
//! `designs/2026-06-06-exchange-ceremony-design.md`): every transport's
//! validated success emits `Command::Celebrate` exactly once — never on
//! failure — and the command is byte-identical regardless of auth mode
//! (ADR-032 duress parity: emission sites never consult it; the exact
//! serialization is pinned here so any conditioning would break the pin).

// `AppScreen`/`UserAction`/`WorkflowEngine` are used only by the
// testing-gated multi-stage module below — imported there, so the
// featureless `--all-targets` clippy (CI lint:clippy) stays clean.
use vauchi_app::ui::AppEngine;
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::platform::{AnimationToken, BleLinkDirection, HapticPattern, SoundToken};

fn engine_named(name: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    AppEngine::new(vauchi)
}

fn token_of(engine: &AppEngine) -> Vec<u8> {
    engine
        .vauchi()
        .identity()
        .expect("identity")
        .signing_public_key()
        .to_vec()
}

fn is_celebrate(cmd: &vauchi_core::Command) -> bool {
    matches!(cmd, vauchi_core::Command::Celebrate { .. })
}

// The full two-party BLE exchange: exactly one Celebrate per side, after
// the validated (persisted) success.
// @scenario: exchange :: BLE success celebrates exactly once
// @internal
#[test]
fn ble_success_celebrates_exactly_once_per_side() {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");
    let alice_token = token_of(&alice);
    let bob_token = token_of(&bob);
    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);
    let ea = alice.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    bob.apply_ble_machine_event(eb);

    let mut alice_celebrates = 0;
    let mut bob_celebrates = 0;
    for _ in 0..50 {
        let mut routed = 0;
        for cmd in alice.drain_pending_commands() {
            if is_celebrate(&cmd) {
                alice_celebrates += 1;
            }
            if let vauchi_core::Command::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            } = cmd
            {
                routed += 1;
                let ev = bob.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id,
                    uuid,
                    data,
                });
                bob.apply_ble_machine_event(ev);
            }
        }
        for cmd in bob.drain_pending_commands() {
            if is_celebrate(&cmd) {
                bob_celebrates += 1;
            }
            if let vauchi_core::Command::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            } = cmd
            {
                routed += 1;
                let ev = alice.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id,
                    uuid,
                    data,
                });
                alice.apply_ble_machine_event(ev);
            }
        }
        if routed == 0 {
            break;
        }
    }
    assert!(
        !alice.vauchi().list_contacts().unwrap().is_empty(),
        "exchange must complete for the assertion to mean anything"
    );
    assert_eq!(alice_celebrates, 1, "Alice celebrates exactly once");
    assert_eq!(bob_celebrates, 1, "Bob celebrates exactly once");
}

// A machine-level BLE failure must never celebrate.
// @scenario: exchange :: BLE failure never celebrates
// @internal
#[test]
fn ble_failure_never_celebrates() {
    let mut alice = engine_named("Alice");
    let bob_token = token_of(&alice); // self-token: never completes
    alice.start_ble_handshake_on_discovery(&bob_token);
    let ev = alice.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "x".into(),
        // TODO(f0-direction): verify — self-token degenerate case, role is incidental.
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ev);
    // Force the machine's failure path via a disconnect mid-flow.
    let ev = alice.forward_ble_hardware_event(&Event::BleDisconnected {
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
        reason: "gone".into(),
    });
    alice.apply_ble_machine_event(ev);
    let celebrates = alice
        .drain_pending_commands()
        .iter()
        .filter(|c| is_celebrate(c))
        .count();
    assert_eq!(celebrates, 0, "failure must not celebrate");
}

// The ceremony payload is one fixed intent triple — pinned bytes, so it
// cannot be conditioned on auth mode (ADR-032 parity) or anything else.
// @internal
#[test]
fn celebrate_serialization_is_pinned() {
    let cmd = vauchi_core::Command::Celebrate {
        haptic: HapticPattern::Success,
        sound: SoundToken::ExchangeChime,
        animation: AnimationToken::CardsMeet,
    };
    let json = serde_json::to_string(&cmd).expect("serialize");
    assert_eq!(
        json,
        r#"{"Celebrate":{"haptic":"success","sound":"exchange_chime","animation":"cards_meet"}}"#,
        "the ceremony wire form is fixed (duress parity: identical bytes always)"
    );
}

// Multi-stage (deterministic two-party drive): exactly one Celebrate per
// side once Finalized persists the contact.
// @scenario: exchange :: multi-stage success celebrates exactly once
#[cfg(feature = "testing")]
mod multi_stage {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};
    use vauchi_app::ui::{AppScreen, Component, UserAction, WorkflowEngine};
    use vauchi_core::clock::{Clock, FakeClock};

    fn own_qr_data(engine: &AppEngine) -> Option<String> {
        engine
            .current_screen()
            .components
            .iter()
            .find_map(|c| match c {
                Component::QrCode { id, data, .. } if id == "own_qr" => Some(data.clone()),
                _ => None,
            })
    }

    fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
        let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
        vauchi.create_identity(name).expect("identity");
        let mut engine = AppEngine::new(vauchi);
        engine.navigate_to(AppScreen::Exchange);
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:hover".into(),
        });
        engine
    }

    fn scan_into(engine: &mut AppEngine, qr: String) {
        let event = engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: qr });
        engine.apply_multi_stage_event(event);
    }

    // @internal
    #[test]
    fn multi_stage_success_celebrates_exactly_once_per_side() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let fake_a = Arc::new(FakeClock::new(start));
        let fake_b = Arc::new(FakeClock::new(start));
        let mut alice = engine_on_hover("Alice", fake_a.clone());
        let mut bob = engine_on_hover("Bob", fake_b.clone());

        let mut alice_celebrates = 0;
        let mut bob_celebrates = 0;
        for _ in 0..600 {
            alice.poll_notifications();
            bob.poll_notifications();
            alice_celebrates += alice
                .drain_pending_commands()
                .iter()
                .filter(|c| is_celebrate(c))
                .count();
            bob_celebrates += bob
                .drain_pending_commands()
                .iter()
                .filter(|c| is_celebrate(c))
                .count();
            let a_qr = own_qr_data(&alice);
            let b_qr = own_qr_data(&bob);
            if let Some(d) = b_qr {
                scan_into(&mut alice, d);
            }
            if let Some(d) = a_qr {
                scan_into(&mut bob, d);
            }
            fake_a.advance(Duration::from_millis(500));
            fake_b.advance(Duration::from_millis(500));
            if !alice.vauchi().list_contacts().unwrap().is_empty()
                && !bob.vauchi().list_contacts().unwrap().is_empty()
            {
                break;
            }
        }
        // Drain what the finalize itself queued.
        alice_celebrates += alice
            .drain_pending_commands()
            .iter()
            .filter(|c| is_celebrate(c))
            .count();
        bob_celebrates += bob
            .drain_pending_commands()
            .iter()
            .filter(|c| is_celebrate(c))
            .count();

        assert!(
            !alice.vauchi().list_contacts().unwrap().is_empty(),
            "multi-stage exchange must finalize"
        );
        assert_eq!(alice_celebrates, 1, "Alice celebrates exactly once");
        assert_eq!(bob_celebrates, 1, "Bob celebrates exactly once");
    }
}

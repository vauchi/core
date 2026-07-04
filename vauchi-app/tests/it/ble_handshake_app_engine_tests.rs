// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level integration tests for the BLE handshake machine
//! (slice 32m T2.2b).
//!
//! Pins the 5-method seam (`ensure_*`, `cancel_*`,
//! `forward_*_hardware_event`, `*_session_active`, `*_phase`)
//! against the same shape multi-stage's
//! `app_engine/multi_stage_exchange.rs` exposes, so cabi/windows
//! can drive BLE through a uniform AppEngine surface.

use vauchi_app::orchestrator::ble_handshake_machine::{BleMachinePhase, BleRole};
use vauchi_app::ui::AppEngine;
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::BleCardPayload;

fn fresh_engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    AppEngine::new(vauchi)
}

fn fixture_card() -> (BleCardPayload, [u8; 32], X3DHKeyPair) {
    let identity_key = [3u8; 32];
    let exchange_secret = [4u8; 32];
    let x3dh = X3DHKeyPair::from_bytes(exchange_secret);
    let exchange_pub = *x3dh.public_key();
    let card = BleCardPayload::new(identity_key, "Bob".into(), exchange_pub, vec![], None);
    (card, identity_key, x3dh)
}

// @internal
#[test]
fn fresh_engine_has_no_active_ble_session() {
    let engine = fresh_engine();
    assert!(!engine.ble_handshake_session_active());
    assert!(engine.ble_machine_phase().is_none());
}

// @internal
#[test]
fn ensure_then_cancel_round_trip() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id, x3dh, card, None);
    assert!(engine.ble_handshake_session_active());
    assert_eq!(engine.ble_machine_phase(), Some(BleMachinePhase::Preparing));

    engine.cancel_ble_handshake_session();
    assert!(!engine.ble_handshake_session_active());
    assert!(engine.ble_machine_phase().is_none());
}

// @internal
#[test]
fn ensure_is_idempotent_under_repeat() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Responder, id, x3dh, card, None);
    let phase_a = engine.ble_machine_phase();

    // Second call with a different role must be a no-op — the
    // already-held session wins. X3DHKeyPair has no Clone, so we
    // rebuild from the same secret bytes the fixture used.
    let (card2, id2, x3dh2) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id2, x3dh2, card2, None);
    let phase_b = engine.ble_machine_phase();
    assert_eq!(phase_a, phase_b);
}

// @internal
#[test]
fn forward_mtu_negotiated_updates_machine_without_touching_phase() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id, x3dh, card, None);
    let _ = engine.forward_ble_hardware_event(&Event::BleMtuNegotiated {
        device_id: "AA:BB".into(),
        mtu: 247,
    });
    // Phase stays Preparing (no protocol transition); the MTU is
    // recorded inside the machine's chunker (read indirectly when
    // T2.2c emits chunked writes).
    assert_eq!(engine.ble_machine_phase(), Some(BleMachinePhase::Preparing));
    assert!(engine.ble_handshake_session_active());
}

// @internal
#[test]
fn forward_ble_connected_advances_initiator_to_handshaking() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id, x3dh, card, None);
    let _ = engine.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "d1".into(),
    });
    assert_eq!(
        engine.ble_machine_phase(),
        Some(BleMachinePhase::Handshaking),
        "initiator must reach Handshaking after BleConnected",
    );
    let pending = engine.drain_pending_commands();
    assert!(
        !pending.is_empty(),
        "BleConnected on initiator must enqueue the KeyOffer command",
    );
}

// @internal
#[test]
fn forward_event_without_active_session_is_no_op() {
    let mut engine = fresh_engine();
    // No ensure_* call — session slot is empty.
    let event = Event::BleConnected {
        device_id: "ghost".into(),
    };
    let _ = engine.forward_ble_hardware_event(&event);
    assert!(!engine.ble_handshake_session_active());
    assert!(engine.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn cancel_enqueues_ble_disconnect_into_pending_commands() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id, x3dh, card, None);
    // Drive past Preparing so cancel emits BleDisconnect.
    let _ = engine.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "d1".into(),
    });
    let _ = engine.drain_pending_commands(); // clear the KeyOffer

    engine.cancel_ble_handshake_session();
    let pending = engine.drain_pending_commands();
    assert_eq!(pending.len(), 1);
    assert!(matches!(pending[0], vauchi_core::Command::BleDisconnect));
}

// @internal
#[test]
fn forward_disconnect_marks_machine_failed() {
    let mut engine = fresh_engine();
    let (card, id, x3dh) = fixture_card();
    engine.ensure_ble_handshake_session(BleRole::Initiator, id, x3dh, card, None);
    let _ = engine.forward_ble_hardware_event(&Event::BleDisconnected {
        reason: "peer left".into(),
    });
    assert!(matches!(
        engine.ble_machine_phase(),
        Some(BleMachinePhase::Failed { .. })
    ));
}

// ── P2: session built on discovery, role from the tiebreak token ──────

// @internal
#[test]
fn discovery_larger_peer_token_starts_initiator_session() {
    // The peer advertises a token that sorts above this device's 32-byte
    // identity key (0xFF * 33 ≥ any 32-byte value), so we win the
    // tiebreak and start as initiator — derived from the live identity,
    // not a fixture card.
    let mut engine = fresh_engine();
    assert!(!engine.ble_handshake_session_active());

    engine.start_ble_handshake_on_discovery(&[0xFF; 33]);
    assert!(
        engine.ble_handshake_session_active(),
        "discovery must build the handshake session",
    );

    let _ = engine.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "d1".into(),
    });
    assert_eq!(
        engine.ble_machine_phase(),
        Some(BleMachinePhase::Handshaking),
        "initiator role must advance to Handshaking on connect",
    );
    assert!(
        !engine.drain_pending_commands().is_empty(),
        "initiator must enqueue the KeyOffer on connect",
    );
}

// @internal
#[test]
fn discovery_smaller_peer_token_starts_responder_session() {
    // An empty peer token sorts below our identity key, so we are the
    // responder and must wait for the initiator's KeyOffer write rather
    // than advancing on connect.
    let mut engine = fresh_engine();
    engine.start_ble_handshake_on_discovery(&[]);
    assert!(engine.ble_handshake_session_active());

    let _ = engine.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "d1".into(),
    });
    // Both roles reach Handshaking on connect; the role differentiator is
    // that the responder emits NO KeyOffer — it waits for the initiator's
    // KeyOffer write (see `on_connected`). The initiator case
    // (discovery_larger_peer_token_…) asserts the complementary command.
    assert!(
        engine.drain_pending_commands().is_empty(),
        "responder must not emit a KeyOffer on connect",
    );
}

// @internal
#[test]
fn discovery_is_idempotent_once_session_active() {
    let mut engine = fresh_engine();
    engine.start_ble_handshake_on_discovery(&[0xFF; 33]);
    let phase_a = engine.ble_machine_phase();
    // A second discovery with a different token must not rebuild the
    // already-held session.
    engine.start_ble_handshake_on_discovery(&[]);
    assert_eq!(engine.ble_machine_phase(), phase_a);
}

// ── P3: two-party exchange persists each other's contact + ratchet ────

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

/// Route one side's pending BLE writes to the other as notifications on
/// the same characteristic — a GATT write on uuid X surfaces at the peer
/// as data on uuid X — applying any resulting machine event (which
/// persists the contact on `Completed`). Returns the number of writes
/// routed, so the caller can pump to a fixpoint.
fn pump(from: &mut AppEngine, to: &mut AppEngine) -> usize {
    let mut routed = 0;
    for cmd in from.drain_pending_commands() {
        if let vauchi_core::Command::BleWriteCharacteristic { uuid, data } = cmd {
            routed += 1;
            let ev =
                to.forward_ble_hardware_event(&Event::BleCharacteristicNotified { uuid, data });
            to.apply_ble_machine_event(ev);
        }
    }
    routed
}

/// Like `pump`, but captures the non-BLE commands the sender emits, so a
/// test can assert e.g. the capture-at-exchange `LocationRequest` queued at
/// finalize (which plain `pump` would silently drop).
fn pump_capture(
    from: &mut AppEngine,
    to: &mut AppEngine,
    captured: &mut Vec<vauchi_core::Command>,
) -> usize {
    let mut routed = 0;
    for cmd in from.drain_pending_commands() {
        if let vauchi_core::Command::BleWriteCharacteristic { uuid, data } = &cmd {
            routed += 1;
            let ev = to.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                uuid: uuid.clone(),
                data: data.clone(),
            });
            to.apply_ble_machine_event(ev);
        } else {
            captured.push(cmd);
        }
    }
    routed
}

// @internal
#[test]
fn two_party_ble_exchange_persists_each_others_contact() {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");
    let alice_token = token_of(&alice);
    let bob_token = token_of(&bob);

    // Each discovers the other → builds a session with the tiebreak role.
    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);
    assert!(alice.ble_handshake_session_active());
    assert!(bob.ble_handshake_session_active());

    // Connect both; the initiator emits its KeyOffer on connect.
    let ea = alice.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "bob".into(),
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "alice".into(),
    });
    bob.apply_ble_machine_event(eb);

    // Pump writes back and forth until the exchange settles, capturing the
    // non-BLE commands Alice emits.
    let mut alice_emitted: Vec<vauchi_core::Command> = Vec::new();
    for _ in 0..50 {
        let a = pump_capture(&mut alice, &mut bob, &mut alice_emitted);
        let b = pump(&mut bob, &mut alice);
        if a + b == 0 {
            break;
        }
    }

    let alice_contacts = alice.vauchi().list_contacts().expect("list");
    let bob_contacts = bob.vauchi().list_contacts().expect("list");
    assert_eq!(alice_contacts.len(), 1, "Alice should have exactly Bob");
    assert_eq!(alice_contacts[0].display_name(), "Bob");
    assert_eq!(bob_contacts.len(), 1, "Bob should have exactly Alice");
    assert_eq!(bob_contacts[0].display_name(), "Alice");

    // ADR-051 capture-at-exchange: the in-person BLE finalize requests a
    // location fix (same seam as multi-stage).
    assert!(
        alice_emitted
            .iter()
            .any(|c| matches!(c, vauchi_core::Command::LocationRequest { .. })),
        "BLE finalize should queue Command::LocationRequest, got {alice_emitted:?}"
    );
}

// P1 step 2a+2b (full loop): a BLE exchange stamps the contact Pending, emits
// the post-persist confirmation ack (G1), the peer verifies it, and reciprocity
// resolves to Confirmed on both sides — relay-free over the native channel.
// @internal
#[test]
fn two_party_ble_exchange_confirms_reciprocity_both_sides() {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");
    let alice_token = token_of(&alice);
    let bob_token = token_of(&bob);

    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);
    let ea = alice.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "bob".into(),
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "alice".into(),
    });
    bob.apply_ble_machine_event(eb);

    // Pump to completion, capturing every write Alice routes so the
    // post-persist ack is observable.
    let mut alice_writes: Vec<Vec<u8>> = Vec::new();
    for _ in 0..50 {
        let mut a = 0;
        for cmd in alice.drain_pending_commands() {
            if let vauchi_core::Command::BleWriteCharacteristic { uuid, data } = cmd {
                alice_writes.push(data.clone());
                let ev = bob
                    .forward_ble_hardware_event(&Event::BleCharacteristicNotified { uuid, data });
                bob.apply_ble_machine_event(ev);
                a += 1;
            }
        }
        let b = pump(&mut bob, &mut alice);
        if a + b == 0 {
            break;
        }
    }

    let now = alice.vauchi().clock().unix_seconds();
    let alice_contacts = alice.vauchi().list_contacts().expect("list");
    assert_eq!(alice_contacts.len(), 1, "Alice persisted Bob");
    assert_eq!(
        alice_contacts[0].reciprocity(now),
        vauchi_core::exchange::reciprocity::Reciprocity::Confirmed,
        "after the ack round-trip the BLE contact must resolve to Confirmed"
    );

    // The reciprocity ack is our version byte + the AEAD sealing of the 32-byte
    // token (enc-version 1 + nonce 24 + ct 32 + tag 16) = 74 bytes, distinct
    // from the handshake frames — emitted only after the contact persisted
    // (structural: same Completed arm, post-persist).
    const ACK_LEN: usize = 1 + (1 + 24 + 32 + 16);
    assert!(
        alice_writes.iter().any(|w| w.len() == ACK_LEN),
        "Alice must emit the post-persist reciprocity ack ({ACK_LEN}B); write sizes: {:?}",
        alice_writes.iter().map(|w| w.len()).collect::<Vec<_>>()
    );
}

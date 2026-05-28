// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Behaviour tests for the BLE handshake machine (slice 32m T2.2a).
//!
//! Mirrors the test patterns established in
//! `multi_stage_machine_proptest.rs`:
//!
//! - Mode + role gating (Initiator vs Responder)
//! - Terminal absorption (Failed / Cancelled)
//! - Command shape: `BleWriteCharacteristic` on the correct
//!   characteristic UUID at the correct phase
//! - MTU update is idempotent under re-negotiation
//! - Cancel emits `BleDisconnect` exactly once
//!
//! The deep four-phase happy-path exchange is covered by the
//! existing `vauchi-core/tests/it/ble_proptest.rs` proptest
//! (`prop_chunking_roundtrip` etc.) which exercises the same
//! `BleHandshakeSession` + `BleChunker` types the machine owns.

use vauchi_app::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase, BleRole,
};
use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    BLE_DEFAULT_USABLE, BleCardPayload, CHAR_DATA_WRITE, CHAR_HANDSHAKE_WRITE,
};

fn fixture_card() -> (BleCardPayload, [u8; 32], X3DHKeyPair) {
    let identity_key = [1u8; 32];
    let exchange_secret = [2u8; 32];
    let x3dh = X3DHKeyPair::from_bytes(exchange_secret);
    let exchange_pub = *x3dh.public_key();
    let card = BleCardPayload::new(
        identity_key,
        "Alice".into(),
        exchange_pub,
        vec![("email".into(), "alice@example.test".into())],
        None,
    );
    (card, identity_key, x3dh)
}

fn fresh_initiator() -> BleHandshakeMachine {
    let (card, id, x3dh) = fixture_card();
    BleHandshakeMachine::new_initiator(id, x3dh, card, 0)
}

fn fresh_responder() -> BleHandshakeMachine {
    let (card, id, x3dh) = fixture_card();
    BleHandshakeMachine::new_responder(id, x3dh, card, 0)
}

// @internal
#[test]
fn initiator_constructs_in_preparing_phase() {
    let m = fresh_initiator();
    assert_eq!(m.phase(), BleMachinePhase::Preparing);
    assert_eq!(m.role(), BleRole::Initiator);
    assert!(!m.is_terminal());
}

// @internal
#[test]
fn responder_constructs_in_preparing_phase() {
    let m = fresh_responder();
    assert_eq!(m.phase(), BleMachinePhase::Preparing);
    assert_eq!(m.role(), BleRole::Responder);
    assert!(!m.is_terminal());
}

// @internal
#[test]
fn initiator_on_connected_emits_key_offer_on_handshake_write() {
    let mut m = fresh_initiator();
    let (event, cmds) = m.on_connected(0);
    assert!(matches!(event, BleMachineEvent::None));
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::BleWriteCharacteristic { uuid, data } => {
            assert_eq!(uuid, CHAR_HANDSHAKE_WRITE);
            assert!(
                !data.is_empty(),
                "KeyOffer payload must not be empty: {cmds:?}",
            );
        }
        other => panic!("expected BleWriteCharacteristic, got {other:?}"),
    }
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// @internal
#[test]
fn responder_on_connected_is_no_op_pending_frontend_subscribe() {
    let mut m = fresh_responder();
    let (event, cmds) = m.on_connected(0);
    assert!(matches!(event, BleMachineEvent::None));
    assert!(
        cmds.is_empty(),
        "responder must not emit any subscribe Command (T0.2 \u{00a7}3.1 hypothesis); got {cmds:?}",
    );
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// @internal
#[test]
fn happy_path_emits_no_subscribe_notify_command() {
    // Drive both an initiator and a responder through their
    // on_connected handler; assert that no emitted `Command`'s
    // `variant_name()` contains "SubscribeNotify".
    //
    // T0.2 §3.1 hypothesis verification: frontends auto-subscribe
    // on connect; the machine never emits a subscribe Command. If
    // a future change adds a SubscribeNotify-shaped Command, this
    // test fails and T3.1's retire-`mobile_ble::subscribe_notify`
    // step must add a Command variant first.
    let mut emitted: Vec<Command> = Vec::new();

    let mut init = fresh_initiator();
    let (_, mut cmds) = init.on_connected(0);
    emitted.append(&mut cmds);

    let mut resp = fresh_responder();
    let (_, mut cmds) = resp.on_connected(0);
    emitted.append(&mut cmds);

    for cmd in &emitted {
        let name = cmd.variant_name();
        assert!(
            !name.contains("SubscribeNotify"),
            "happy-path emitted a SubscribeNotify-shaped command ({name})",
        );
    }
}

// @internal
#[test]
fn update_mtu_is_idempotent_under_renegotiation() {
    let mut m = fresh_initiator();
    assert_eq!(m.mtu_usable(), BLE_DEFAULT_USABLE);
    m.update_mtu(247);
    assert_eq!(m.mtu_usable(), 247 - 3);
    // Re-negotiate higher
    m.update_mtu(517);
    assert_eq!(m.mtu_usable(), 517 - 3);
    // Re-negotiate lower
    m.update_mtu(100);
    assert_eq!(m.mtu_usable(), 100 - 3);
}

// @internal
#[test]
fn update_mtu_clamps_above_chunk_overhead() {
    let mut m = fresh_initiator();
    // A peer reporting `mtu = 0` (unrealistic but defensive) must
    // not produce a degenerate chunker that emits zero-byte chunks.
    m.update_mtu(0);
    assert!(
        m.mtu_usable() > vauchi_core::exchange::BLE_CHUNK_OVERHEAD,
        "mtu_usable must stay above BLE_CHUNK_OVERHEAD even for absurd MTU; got {}",
        m.mtu_usable(),
    );
}

// @internal
#[test]
fn cancel_emits_ble_disconnect_once_and_is_absorbing() {
    let mut m = fresh_initiator();
    m.on_connected(0);

    let cmds = m.cancel();
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::BleDisconnect));
    assert_eq!(m.phase(), BleMachinePhase::Cancelled);
    assert!(m.is_terminal());

    // Second cancel is a no-op.
    let cmds = m.cancel();
    assert!(
        cmds.is_empty(),
        "second cancel must not re-emit BleDisconnect: {cmds:?}",
    );

    // Further ingress is inert.
    let (event, cmds) = m.on_disconnected("late");
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
    assert_eq!(m.phase(), BleMachinePhase::Cancelled);
}

// @internal
#[test]
fn disconnected_fails_the_machine_with_reason() {
    let mut m = fresh_initiator();
    m.on_connected(0);
    let (event, cmds) = m.on_disconnected("timeout");
    match event {
        BleMachineEvent::Failed { reason } => {
            assert!(
                reason.contains("BLE disconnected"),
                "failure reason must mention BLE disconnection: {reason}",
            );
            assert!(reason.contains("timeout"), "reason must carry the cause");
        }
        other => panic!("expected Failed event, got {other:?}"),
    }
    assert!(cmds.is_empty());
    assert!(matches!(m.phase(), BleMachinePhase::Failed { .. }));
    assert!(m.is_terminal());
}

// @internal
#[test]
fn unknown_characteristic_uuid_is_inert() {
    let mut m = fresh_initiator();
    m.on_connected(0);
    let (event, cmds) = m.on_data_received("not-a-real-uuid", &[0xAA; 16], 0);
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// @internal
#[test]
fn data_chunk_smaller_than_overhead_is_inert() {
    let mut m = fresh_initiator();
    m.on_connected(0);
    let (event, cmds) = m.on_data_received(CHAR_DATA_WRITE, &[0xAA, 0xBB], 0);
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
}

// @internal
#[test]
fn role_is_stable_through_lifecycle() {
    let mut init = fresh_initiator();
    init.on_connected(0);
    init.update_mtu(247);
    init.on_data_received("noise", &[], 0);
    assert_eq!(init.role(), BleRole::Initiator);

    let mut resp = fresh_responder();
    resp.on_connected(0);
    resp.update_mtu(247);
    assert_eq!(resp.role(), BleRole::Responder);
}

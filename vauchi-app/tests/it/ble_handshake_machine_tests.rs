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
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase, BleOobBinding, BleRole,
};
use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    BLE_DEFAULT_USABLE, BleCardPayload, CHAR_DATA_NOTIFY, CHAR_DATA_WRITE, CHAR_HANDSHAKE_NOTIFY,
    CHAR_HANDSHAKE_WRITE,
};
use vauchi_core::platform::BleLinkDirection;

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
    BleHandshakeMachine::new_initiator(id, x3dh, card, 0, None)
}

fn fresh_responder() -> BleHandshakeMachine {
    let (card, id, x3dh) = fixture_card();
    BleHandshakeMachine::new_responder(id, x3dh, card, 0, None)
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
    let (event, cmds) = m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    assert!(matches!(event, BleMachineEvent::None));
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::BleWriteCharacteristic { uuid, data, .. } => {
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

// F0 fix: the physical link direction — not the token tiebreak — decides the
// role. A device the token labelled responder that ends up dialing out (its
// backoff fallback fired under asymmetric discovery) is reconciled to initiator
// and MUST send the KeyOffer, otherwise it waits forever and the exchange
// deadlocks (`2026-07-22-role-tiebreak-and-glare-design.md`).
// @internal
#[test]
fn responder_token_dialing_out_becomes_initiator_and_offers() {
    let mut m = fresh_responder();
    assert_eq!(
        m.role(),
        BleRole::Responder,
        "token tiebreak said responder"
    );
    let (event, cmds) = m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    assert_eq!(
        m.role(),
        BleRole::Initiator,
        "dialing out (Outbound) reconciles the role to initiator",
    );
    assert!(matches!(event, BleMachineEvent::None));
    assert_eq!(cmds.len(), 1, "the reconciled initiator emits its KeyOffer");
    match &cmds[0] {
        Command::BleWriteCharacteristic { uuid, data, .. } => {
            assert_eq!(uuid, CHAR_HANDSHAKE_WRITE);
            assert!(!data.is_empty(), "KeyOffer must not be empty: {cmds:?}");
        }
        other => panic!("expected BleWriteCharacteristic, got {other:?}"),
    }
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// F0 fix (mirror): a device the token labelled initiator that is instead
// connected TO (inbound peripheral link) reconciles to responder and waits for
// the peer's KeyOffer — it must NOT dial its own KeyOffer over a link it never
// opened (the "No connected device" misroute).
// @internal
#[test]
fn initiator_token_connected_to_becomes_responder() {
    let mut m = fresh_initiator();
    assert_eq!(
        m.role(),
        BleRole::Initiator,
        "token tiebreak said initiator"
    );
    let (event, cmds) = m.on_connected("peer-1", BleLinkDirection::Inbound, 0);
    assert_eq!(
        m.role(),
        BleRole::Responder,
        "being connected to (Inbound) reconciles the role to responder",
    );
    assert!(matches!(event, BleMachineEvent::None));
    assert!(
        cmds.is_empty(),
        "the reconciled responder waits for the KeyOffer, emits nothing: {cmds:?}",
    );
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// @internal
#[test]
fn initiator_duplicate_on_connected_is_idempotent_not_failed() {
    // A second BleConnected arrives because the initiator is ALSO a peripheral
    // (its own GATT server accepts the peer's connection). The duplicate must
    // be a no-op — not re-create the KeyOffer and fail with InvalidState, which
    // killed the S7-as-central handshake just as the peer's KeyAck arrived.
    let mut m = fresh_initiator();
    let (_e1, cmds1) = m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    assert_eq!(cmds1.len(), 1, "first connect emits the KeyOffer");
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);

    let (event2, cmds2) = m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    assert!(
        matches!(event2, BleMachineEvent::None),
        "duplicate connect is a no-op event",
    );
    assert!(
        cmds2.is_empty(),
        "duplicate connect must not emit a second KeyOffer: {cmds2:?}",
    );
    assert_eq!(
        m.phase(),
        BleMachinePhase::Handshaking,
        "machine stays Handshaking, not Failed",
    );
    assert!(
        !m.is_terminal(),
        "duplicate connect must not fail the machine"
    );
}

// @internal
#[test]
fn responder_on_connected_is_no_op_pending_frontend_subscribe() {
    let mut m = fresh_responder();
    let (event, cmds) = m.on_connected("peer-1", BleLinkDirection::Inbound, 0);
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
    let (_, mut cmds) = init.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    emitted.append(&mut cmds);

    let mut resp = fresh_responder();
    let (_, mut cmds) = resp.on_connected("peer-1", BleLinkDirection::Inbound, 0);
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
    m.on_connected("peer-1", BleLinkDirection::Outbound, 0);

    let cmds = m.cancel();
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], Command::BleDisconnect { .. }));
    assert_eq!(m.phase(), BleMachinePhase::Cancelled);
    assert!(m.is_terminal());

    // Second cancel is a no-op.
    let cmds = m.cancel();
    assert!(
        cmds.is_empty(),
        "second cancel must not re-emit BleDisconnect: {cmds:?}",
    );

    // Further ingress is inert.
    let (event, cmds) = m.on_disconnected("peer-1", BleLinkDirection::Outbound, "late");
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
    assert_eq!(m.phase(), BleMachinePhase::Cancelled);
}

// @internal
#[test]
fn disconnected_fails_the_machine_with_reason() {
    let mut m = fresh_initiator();
    m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    let (event, cmds) = m.on_disconnected("peer-1", BleLinkDirection::Outbound, "timeout");
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
    m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    let (event, cmds) = m.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        "not-a-real-uuid",
        &[0xAA; 16],
        0,
    );
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
    assert_eq!(m.phase(), BleMachinePhase::Handshaking);
}

// @internal
#[test]
fn data_chunk_smaller_than_overhead_is_inert() {
    let mut m = fresh_initiator();
    m.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    let (event, cmds) = m.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        CHAR_DATA_WRITE,
        &[0xAA, 0xBB],
        0,
    );
    assert!(matches!(event, BleMachineEvent::None));
    assert!(cmds.is_empty());
}

// @internal
#[test]
fn role_is_stable_through_lifecycle() {
    let mut init = fresh_initiator();
    init.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    init.update_mtu(247);
    init.on_data_received("peer-1", BleLinkDirection::Outbound, "noise", &[], 0);
    assert_eq!(init.role(), BleRole::Initiator);

    let mut resp = fresh_responder();
    resp.on_connected("peer-1", BleLinkDirection::Inbound, 0);
    resp.update_mtu(247);
    assert_eq!(resp.role(), BleRole::Responder);
}

fn initiator_with_oob(oob: Option<BleOobBinding>) -> BleHandshakeMachine {
    let (card, id, x3dh) = fixture_card();
    BleHandshakeMachine::new_initiator(id, x3dh, card, 0, oob)
}

/// The KeyOffer bytes the initiator writes on connect.
fn key_offer(machine: &mut BleHandshakeMachine) -> Vec<u8> {
    let (_event, cmds) = machine.on_connected("peer-1", BleLinkDirection::Outbound, 0);
    cmds.into_iter()
        .find_map(|cmd| match cmd {
            Command::BleWriteCharacteristic { uuid, data, .. }
                if uuid.as_str() == CHAR_HANDSHAKE_WRITE =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("initiator emits a KeyOffer on connect")
}

// @internal
#[test]
fn oob_nonce_echo_rides_in_the_key_offer() {
    // Bootstrapped modes: the scanner echoes the OOB nonce it saw so the
    // displayer can verify the connector actually saw the QR/tap. It occupies
    // KeyOffer bytes 121..137 (after version/identity/exchange/ephemeral/
    // nonce/timestamp).
    let nonce = [7u8; 16];
    let offer = key_offer(&mut initiator_with_oob(Some(BleOobBinding {
        oob_nonce_echo: Some(nonce),
        ..Default::default()
    })));
    assert_eq!(
        &offer[121..137],
        &nonce,
        "set_oob_nonce must be threaded into the KeyOffer's echo field"
    );
}

// @internal
#[test]
fn radio_only_binding_leaves_the_echo_zero() {
    // Control: `None` (radio-only Magic/Bump/Shake) has no OOB peer, so the
    // echo field stays all-zero. Guards against the threading firing
    // unconditionally.
    let offer = key_offer(&mut initiator_with_oob(None));
    assert_eq!(
        &offer[121..137],
        &[0u8; 16],
        "no OOB binding must leave the echo field zero"
    );
}

/// A responder with an identity distinct from `fixture_card`'s, so the
/// initiator's self-exchange check does not reject the pair.
fn fresh_peer_responder() -> BleHandshakeMachine {
    let identity_key = [3u8; 32];
    let x3dh = X3DHKeyPair::from_bytes([4u8; 32]);
    let card = BleCardPayload::new(
        identity_key,
        "Bob".into(),
        *x3dh.public_key(),
        vec![("email".into(), "bob@example.test".into())],
        None,
    );
    BleHandshakeMachine::new_responder(identity_key, x3dh, card, 0, None)
}

/// A peer initiator with an identity ([3;32]) larger than `fixture_card`'s
/// ([1;32]) — for the glare tiebreak.
fn fresh_peer_initiator() -> BleHandshakeMachine {
    let identity_key = [3u8; 32];
    let x3dh = X3DHKeyPair::from_bytes([4u8; 32]);
    let card = BleCardPayload::new(
        identity_key,
        "Bob".into(),
        *x3dh.public_key(),
        vec![("email".into(), "bob@example.test".into())],
        None,
    );
    BleHandshakeMachine::new_initiator(identity_key, x3dh, card, 0, None)
}

// Symmetric-discovery glare (device-confirmed): both peers initiated, so each
// receives the other's KeyOffer on CHAR_HANDSHAKE_WRITE while already an
// initiator. The identity-key tiebreak must make the LARGER yield to responder
// (and emit a KeyAck) and the SMALLER stay initiator (ignore) — otherwise both
// ignore and the exchange stalls.
// @internal
#[test]
fn glare_larger_identity_yields_to_responder_smaller_stays_initiator() {
    let mut alice = fresh_initiator(); // identity [1;32] — smaller
    let mut bob = fresh_peer_initiator(); // identity [3;32] — larger
    let offer_a = key_offer(&mut alice);
    let offer_b = key_offer(&mut bob);
    assert_eq!(alice.role(), BleRole::Initiator);
    assert_eq!(bob.role(), BleRole::Initiator);

    // Smaller identity receives the peer's KeyOffer → stays initiator, ignores.
    let (_ea, cmds_a) = alice.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &offer_b,
        0,
    );
    assert_eq!(
        alice.role(),
        BleRole::Initiator,
        "smaller identity must stay initiator",
    );
    assert!(
        cmds_a.is_empty(),
        "smaller identity ignores the peer's KeyOffer, emits nothing: {cmds_a:?}",
    );

    // Larger identity receives the peer's KeyOffer → yields to responder + KeyAck.
    let (_eb, cmds_b) = bob.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &offer_a,
        0,
    );
    assert_eq!(
        bob.role(),
        BleRole::Responder,
        "larger identity must yield to responder",
    );
    assert!(
        cmds_b.iter().any(|c| matches!(
            c,
            Command::BleWriteCharacteristic { uuid, .. } if uuid == CHAR_HANDSHAKE_NOTIFY
        )),
        "yielded responder emits a KeyAck on the handshake-notify char: {cmds_b:?}",
    );
}

// Glare hardening (design §synthesized final): the yielding side holds TWO
// links — its own outbound (it dialed) and the inbound the peer's KeyOffer
// arrived on. Yielding must (a) ride the inbound link from then on and (b)
// drop the losing outbound link with a targeted BleDisconnect — leaving it
// idle leaks a half-open link the peer writes into until supervision
// timeout (design rejection F).
// @internal
#[test]
fn glare_yield_drops_the_losing_outbound_link_and_rides_inbound() {
    let mut alice = fresh_initiator(); // identity [1;32] — smaller
    let mut bob = fresh_peer_initiator(); // identity [3;32] — larger
    let offer_a = key_offer_on(&mut alice, "a-out");
    let _offer_b = key_offer_on(&mut bob, "b-out");

    // Bob (larger) receives Alice's KeyOffer on his INBOUND link.
    let (_eb, cmds_b) = bob.on_data_received(
        "b-in",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &offer_a,
        0,
    );
    assert_eq!(bob.role(), BleRole::Responder, "larger identity yields");
    assert!(
        cmds_b.iter().any(|c| matches!(
            c,
            Command::BleDisconnect { device_id, direction }
                if device_id == "b-out" && *direction == BleLinkDirection::Outbound
        )),
        "yielding must drop the losing outbound link with a targeted \
         disconnect: {cmds_b:?}",
    );
    assert!(
        cmds_b.iter().any(|c| matches!(
            c,
            Command::BleWriteCharacteristic { device_id, uuid, .. }
                if uuid == CHAR_HANDSHAKE_NOTIFY && device_id == "b-in"
        )),
        "the KeyAck must ride the surviving inbound link: {cmds_b:?}",
    );
}

// After glare where this side STAYS initiator (smaller identity), events
// arriving on the losing inbound link are stale — feeding them into the
// surviving session would corrupt it (design: reject stale-device_id
// events).
// @internal
#[test]
fn stale_link_data_is_rejected_after_glare_stay() {
    let mut alice = fresh_initiator(); // smaller — stays initiator
    let mut bob = fresh_peer_initiator();
    let offer_a = key_offer_on(&mut alice, "a-out");
    let offer_b = key_offer_on(&mut bob, "b-out");

    let (_ea, _cmds) = alice.on_data_received(
        "a-in",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &offer_b,
        0,
    );
    assert_eq!(alice.role(), BleRole::Initiator, "smaller identity stays");

    // A (valid-shaped) KeyAck arriving on the stale inbound link must be
    // ignored — not buffered, not a phase flip.
    let ack = key_ack_from_responder(&offer_a);
    let (event, cmds) = alice.on_data_received(
        "a-in",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::None),
        "KeyAck on the stale link must be ignored",
    );
    assert!(cmds.is_empty(), "no commands from a stale-link frame");
    assert!(
        matches!(alice.phase(), BleMachinePhase::Handshaking),
        "phase must not advance on a stale-link KeyAck",
    );

    // The same KeyAck on the ACTIVE link processes normally.
    let (event, _cmds) = alice.on_data_received(
        "a-out",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::TransferringStarted),
        "KeyAck on the active link must advance the handshake",
    );
}

// Android identifies both GATT directions with the same Bluetooth address.
// Direction must therefore participate in the link identity; device_id alone
// cannot distinguish the surviving outbound link from the stale inbound one.
// @internal
#[test]
fn stale_same_peer_data_is_rejected_by_direction_after_glare_stay() {
    let mut alice = fresh_initiator(); // smaller — stays initiator
    let mut bob = fresh_peer_initiator();
    let offer_a = key_offer_on(&mut alice, "peer");
    let offer_b = key_offer_on(&mut bob, "peer");

    let (_event, _commands) = alice.on_data_received(
        "peer",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &offer_b,
        0,
    );
    assert_eq!(alice.role(), BleRole::Initiator, "smaller identity stays");

    let ack = key_ack_from_responder(&offer_a);
    let (event, commands) = alice.on_data_received(
        "peer",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::None),
        "KeyAck on the stale direction must be ignored",
    );
    assert!(commands.is_empty(), "no commands from a stale-link frame");
    assert!(
        matches!(alice.phase(), BleMachinePhase::Handshaking),
        "phase must not advance on a stale-direction KeyAck",
    );

    let (event, _commands) = alice.on_data_received(
        "peer",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::TransferringStarted),
        "KeyAck on the active direction must advance the handshake",
    );
}

// A disconnect of a link the machine is NOT riding (the dropped glare
// loser) must not kill the surviving handshake — that phantom-disconnect
// was the reason direction/device_id addressing exists at all.
// @internal
#[test]
fn disconnect_of_non_active_link_does_not_fail_the_machine() {
    let mut m = fresh_initiator();
    let _offer = key_offer_on(&mut m, "out-1");

    let (event, cmds) = m.on_disconnected("in-1", BleLinkDirection::Inbound, "glare loser dropped");
    assert!(
        matches!(event, BleMachineEvent::None),
        "non-active-link disconnect must be a no-op",
    );
    assert!(cmds.is_empty());
    assert!(
        !m.is_terminal(),
        "machine must survive the losing link's disconnect",
    );

    // The ACTIVE link's disconnect still fails the machine, as does a
    // legacy un-addressed one (empty device_id = wildcard).
    let (event, _cmds) = m.on_disconnected("out-1", BleLinkDirection::Outbound, "gone");
    assert!(
        matches!(event, BleMachineEvent::Failed { .. }),
        "active-link disconnect must fail the machine",
    );
}

// Legacy/un-addressed disconnects (empty device_id) keep the old
// fail-the-machine behavior — shells that cannot name the link yet must
// not have their failure reports silently dropped.
// @internal
#[test]
fn unaddressed_disconnect_still_fails_the_machine() {
    let mut m = fresh_initiator();
    let _offer = key_offer_on(&mut m, "out-1");
    let (event, _cmds) = m.on_disconnected("", BleLinkDirection::Outbound, "legacy");
    assert!(
        matches!(event, BleMachineEvent::Failed { .. }),
        "un-addressed disconnect is a wildcard and must still fail",
    );
}

/// Drive `m` to emit its KeyOffer over an outbound link named `link`,
/// returning the offer bytes.
fn key_offer_on(m: &mut BleHandshakeMachine, link: &str) -> Vec<u8> {
    let (_event, cmds) = m.on_connected(link, BleLinkDirection::Outbound, 0);
    cmds.into_iter()
        .find_map(|cmd| match cmd {
            Command::BleWriteCharacteristic { uuid, data, .. }
                if uuid.as_str() == CHAR_HANDSHAKE_WRITE =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("initiator emits a KeyOffer on connect")
}

/// The 153-byte KeyAck a real responder emits on the handshake-notify
/// characteristic after processing `offer`.
fn key_ack_from_responder(offer: &[u8]) -> Vec<u8> {
    let mut responder = fresh_peer_responder();
    let (_event, cmds) = responder.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        offer,
        0,
    );
    cmds.into_iter()
        .find_map(|cmd| match cmd {
            Command::BleWriteCharacteristic { uuid, data, .. }
                if uuid.as_str() == CHAR_HANDSHAKE_NOTIFY =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("responder emits a KeyAck on the handshake-notify characteristic")
}

// @internal
#[test]
fn oversized_handshake_notify_is_inert_not_buffered_as_key_ack() {
    // Input boundary (DC-01): a KeyAck is exactly 153 bytes on the wire. A
    // larger handshake notify is not a KeyAck and must be dropped BEFORE
    // buffering — previously `pending_intermediate` held an attacker-sized
    // Vec until the card chunks completed
    // (backlog/2026-07-20-ble-exchange-orchestrator-unification).
    let mut initiator = fresh_initiator();
    let offer = key_offer(&mut initiator);

    let oversized = vec![0xAAu8; 64 * 1024];
    let (event, cmds) = initiator.on_data_received(
        "peer-1",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &oversized,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::None),
        "an oversized handshake notify must not be treated as a KeyAck"
    );
    assert!(cmds.is_empty(), "no commands from a rejected frame");
    assert!(
        matches!(initiator.phase(), BleMachinePhase::Handshaking),
        "phase must not advance to Transferring on a rejected frame"
    );

    // The machine must still accept the genuine KeyAck afterwards — the
    // rejected frame is quarantine-dropped, not a terminal failure.
    let ack = key_ack_from_responder(&offer);
    assert_eq!(ack.len(), 153, "wire KeyAck is exactly 153 bytes");
    let (event, _cmds) = initiator.on_data_received(
        "peer-1",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::TransferringStarted),
        "the genuine KeyAck must still be accepted after a dropped frame"
    );
    assert!(matches!(initiator.phase(), BleMachinePhase::Transferring));
}

// @internal
#[test]
fn undersized_handshake_notify_is_inert_not_buffered_as_key_ack() {
    // A short frame (radio residue, hostile neighbor) is structurally not a
    // KeyAck: dropped without state advance, same as oversized.
    let mut initiator = fresh_initiator();
    let offer = key_offer(&mut initiator);

    let (event, cmds) = initiator.on_data_received(
        "peer-1",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &[0u8; 20],
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::None),
        "an undersized handshake notify must not be treated as a KeyAck"
    );
    assert!(cmds.is_empty(), "no commands from a rejected frame");
    assert!(
        matches!(initiator.phase(), BleMachinePhase::Handshaking),
        "phase must not advance to Transferring on a rejected frame"
    );

    let ack = key_ack_from_responder(&offer);
    let (event, _cmds) = initiator.on_data_received(
        "peer-1",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::TransferringStarted),
        "the genuine KeyAck must still be accepted after a dropped frame"
    );
}

/// The responder's full Phase-2 output after processing `offer`:
/// `(key_ack, card_chunks_on_data_notify)` in emission order.
fn responder_phase2_output(offer: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut responder = fresh_peer_responder();
    // ATT-minimum MTU (23 → 20 usable) so the encrypted card spans several
    // chunks — the realistic pre-negotiation window where reordering bites.
    responder.update_mtu(23);
    let (_event, cmds) = responder.on_data_received(
        "peer-1",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        offer,
        0,
    );
    let mut ack = None;
    let mut chunks = Vec::new();
    for cmd in cmds {
        if let Command::BleWriteCharacteristic { uuid, data, .. } = cmd {
            match uuid.as_str() {
                u if u == CHAR_HANDSHAKE_NOTIFY => ack = Some(data),
                u if u == CHAR_DATA_NOTIFY => chunks.push(data),
                _ => {}
            }
        }
    }
    (ack.expect("responder emits a KeyAck"), chunks)
}

// @scenario: contact_exchange :: Exchange completes over BLE
/// RED (Phase B, Tier 1): GATT gives no cross-characteristic FIFO, so the
/// responder's card chunks (CHAR_DATA_NOTIFY) can arrive before its KeyAck
/// (CHAR_HANDSHAKE_NOTIFY). The machine must quarantine whichever arrives
/// first and proceed once both are present. Current code fails terminally
/// at card completion ("No pending KeyAck data",
/// `on_remote_encrypted_card_received`) and then drops the late KeyAck at
/// the terminal guard — the observed on-device Magic stall discriminator.
/// Un-ignore when the bounded reorder quarantine ships
/// (`backlog/2026-07-20-ble-exchange-orchestrator-unification`).
// @internal
#[test]
#[ignore = "RED: card-before-KeyAck is terminal — backlog/2026-07-20-ble-exchange-orchestrator-unification"]
fn card_before_key_ack_reorder_should_be_quarantined_not_terminal() {
    let mut initiator = fresh_initiator();
    let offer = key_offer(&mut initiator);
    let (ack, chunks) = responder_phase2_output(&offer);
    assert!(
        chunks.len() > 1,
        "premise: the encrypted card spans multiple chunks at default MTU (got {})",
        chunks.len()
    );

    // Reordered delivery: every card chunk lands before the KeyAck.
    for chunk in &chunks {
        let (_event, cmds) = initiator.on_data_received(
            "peer-1",
            BleLinkDirection::Outbound,
            CHAR_DATA_NOTIFY,
            chunk,
            0,
        );
        assert!(
            !matches!(initiator.phase(), BleMachinePhase::Failed { .. }),
            "card-before-KeyAck must be quarantined, not a terminal failure"
        );
        assert!(cmds.is_empty(), "no commands while the card is quarantined");
    }

    // The late KeyAck completes the pair: the machine processes the
    // quarantined card and advances to Verifying (commitment + our card
    // go out), exactly as in the in-order flow.
    let (event, cmds) = initiator.on_data_received(
        "peer-1",
        BleLinkDirection::Outbound,
        CHAR_HANDSHAKE_NOTIFY,
        &ack,
        0,
    );
    assert!(
        matches!(event, BleMachineEvent::VerifyingStarted),
        "the late KeyAck must unlock processing of the quarantined card"
    );
    assert!(
        matches!(initiator.phase(), BleMachinePhase::Verifying),
        "the reordered flow must reach Verifying like the in-order flow"
    );
    assert!(
        cmds.iter().any(|cmd| matches!(
            cmd,
            Command::BleWriteCharacteristic { uuid, .. } if uuid.as_str() == CHAR_HANDSHAKE_WRITE
        )),
        "the commitment write must be emitted after the reordered pair completes"
    );
}

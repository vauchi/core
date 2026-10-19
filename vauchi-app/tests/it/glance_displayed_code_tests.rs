// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A displayed Glance code is a broadcast: anyone in line of sight can scan
//! it. ADR-053 requires its nonce to be single-use, so a second scanner of
//! the same code gets nothing once the displayer accepted the first
//! (vauchi/private#312, attack 2).

use vauchi_app::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleOobBinding,
};
use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{BleCardPayload, CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE};
use vauchi_core::platform::BleLinkDirection;

const DISPLAYED_NONCE: [u8; 16] = [7u8; 16];

fn party(seed: u8, name: &str) -> ([u8; 32], X3DHKeyPair, BleCardPayload) {
    let identity_key = [seed; 32];
    let x3dh = X3DHKeyPair::from_bytes([seed.wrapping_add(100); 32]);
    let card = BleCardPayload::new(
        identity_key,
        name.into(),
        *x3dh.public_key(),
        vec![("email".into(), format!("{name}@example.test"))],
        None,
    );
    (identity_key, x3dh, card)
}

fn displayer() -> BleHandshakeMachine {
    let (identity_key, x3dh, card) = party(3, "alice");
    BleHandshakeMachine::new_responder(
        identity_key,
        x3dh,
        card,
        0,
        Some(BleOobBinding {
            required_oob_nonce: Some(DISPLAYED_NONCE),
            ..Default::default()
        }),
    )
}

fn scanner_of_the_displayed_code(seed: u8, name: &str) -> BleHandshakeMachine {
    let (identity_key, x3dh, card) = party(seed, name);
    BleHandshakeMachine::new_initiator(
        identity_key,
        x3dh,
        card,
        0,
        Some(BleOobBinding {
            oob_nonce_echo: Some(DISPLAYED_NONCE),
            ..Default::default()
        }),
    )
}

fn key_offer(scanner: &mut BleHandshakeMachine, link: &str) -> Vec<u8> {
    let (_event, commands) = scanner.on_connected(link, BleLinkDirection::Outbound, 0);
    commands
        .into_iter()
        .find_map(|command| match command {
            Command::BleWriteCharacteristic { uuid, data, .. }
                if uuid.as_str() == CHAR_HANDSHAKE_WRITE =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("a scanner sends its KeyOffer on connect")
}

/// The KeyAcks the displayer writes back after receiving `offer` on `link`.
fn key_acks_for(displayer: &mut BleHandshakeMachine, link: &str, offer: &[u8]) -> usize {
    let (_event, commands) = displayer.on_data_received(
        link,
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        offer,
        0,
    );
    commands
        .iter()
        .filter(|command| {
            matches!(command, Command::BleWriteCharacteristic { uuid, .. }
                if uuid.as_str() == CHAR_HANDSHAKE_NOTIFY)
        })
        .count()
}

// @internal
#[test]
fn a_second_scanner_of_one_displayed_code_gets_no_answer() {
    let mut alice = displayer();
    let bob_offer = key_offer(&mut scanner_of_the_displayed_code(1, "bob"), "bob");
    let mallory_offer = key_offer(&mut scanner_of_the_displayed_code(5, "mallory"), "mallory");
    assert_eq!(
        key_acks_for(&mut alice, "bob", &bob_offer),
        1,
        "precondition: the first scanner is answered"
    );

    let answers_to_mallory = key_acks_for(&mut alice, "mallory", &mallory_offer);

    assert_eq!(answers_to_mallory, 0);
}

// @internal
#[test]
fn a_second_scanner_does_not_end_the_first_exchange() {
    let mut alice = displayer();
    let bob_offer = key_offer(&mut scanner_of_the_displayed_code(1, "bob"), "bob");
    let mallory_offer = key_offer(&mut scanner_of_the_displayed_code(5, "mallory"), "mallory");
    key_acks_for(&mut alice, "bob", &bob_offer);

    let (event, _commands) = alice.on_data_received(
        "mallory",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &mallory_offer,
        0,
    );

    assert!(matches!(event, BleMachineEvent::None), "{event:?}");
    assert!(!alice.is_terminal());
}

/// Shells that cannot name the link yet report an empty device id, which the
/// machine treats as a wildcard; the handshake itself must still refuse.
// @internal
#[test]
fn a_second_scanner_on_an_unaddressed_link_gets_no_answer() {
    let mut alice = displayer();
    let bob_offer = key_offer(&mut scanner_of_the_displayed_code(1, "bob"), "");
    let mallory_offer = key_offer(&mut scanner_of_the_displayed_code(5, "mallory"), "");
    assert_eq!(
        key_acks_for(&mut alice, "", &bob_offer),
        1,
        "precondition: the first scanner is answered"
    );

    let answers_to_mallory = key_acks_for(&mut alice, "", &mallory_offer);

    assert_eq!(answers_to_mallory, 0);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A displayed Glance code is a broadcast: anyone in line of sight can scan
//! it. ADR-053 requires its nonce to be single-use, so a second scanner of
//! the same code gets nothing once the displayer accepted the first
//! (vauchi/private#312, attack 2).

use vauchi_app::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleOobBinding,
};
use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    BleCardPayload, CHAR_DATA_NOTIFY, CHAR_DATA_WRITE, CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE,
};
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

fn writes_on(commands: &[Command], characteristic: &str) -> Vec<Vec<u8>> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::BleWriteCharacteristic { uuid, data, .. }
                if uuid.as_str() == characteristic =>
            {
                Some(data.clone())
            }
            _ => None,
        })
        .collect()
}

/// What the displayer reports once the first scanner's card has arrived, when
/// a second scanner's KeyOffer lands between that scanner's commitment and
/// card (vauchi/private#705).
fn exchange_with_a_second_offer_between_commitment_and_card(
    bob_link: &str,
    mallory_link: &str,
) -> BleMachineEvent {
    let mut alice = displayer();
    let mut bob = scanner_of_the_displayed_code(1, "bob");
    let mallory_offer = key_offer(
        &mut scanner_of_the_displayed_code(5, "mallory"),
        mallory_link,
    );
    let bob_offer = key_offer(&mut bob, bob_link);
    let (_event, phase2) = alice.on_data_received(
        bob_link,
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &bob_offer,
        0,
    );
    let mut phase3 = Vec::new();
    for ack in writes_on(&phase2, CHAR_HANDSHAKE_NOTIFY) {
        phase3.extend(
            bob.on_data_received(
                bob_link,
                BleLinkDirection::Outbound,
                CHAR_HANDSHAKE_NOTIFY,
                &ack,
                0,
            )
            .1,
        );
    }
    for chunk in writes_on(&phase2, CHAR_DATA_NOTIFY) {
        phase3.extend(
            bob.on_data_received(
                bob_link,
                BleLinkDirection::Outbound,
                CHAR_DATA_NOTIFY,
                &chunk,
                0,
            )
            .1,
        );
    }
    let commitment = writes_on(&phase3, CHAR_HANDSHAKE_WRITE)
        .pop()
        .expect("bob sends his commitment");
    alice.on_data_received(
        bob_link,
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &commitment,
        0,
    );
    alice.on_data_received(
        mallory_link,
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &mallory_offer,
        0,
    );
    let mut last_event = BleMachineEvent::None;
    for chunk in writes_on(&phase3, CHAR_DATA_WRITE) {
        last_event = alice
            .on_data_received(
                bob_link,
                BleLinkDirection::Inbound,
                CHAR_DATA_WRITE,
                &chunk,
                0,
            )
            .0;
    }
    last_event
}

// @internal
#[test]
fn a_second_scanners_offer_mid_exchange_leaves_the_first_exchange_intact() {
    let event = exchange_with_a_second_offer_between_commitment_and_card("bob", "mallory");

    assert!(matches!(event, BleMachineEvent::Completed(_)), "{event:?}");
}

/// Shells that cannot name the link report an empty device id, which the
/// stale-link gate treats as a wildcard.
// @internal
#[test]
fn a_second_scanners_offer_mid_exchange_on_an_unaddressed_link_leaves_it_intact() {
    let event = exchange_with_a_second_offer_between_commitment_and_card("", "");

    assert!(matches!(event, BleMachineEvent::Completed(_)), "{event:?}");
}

/// Both shells report each inbound connection to the displayer with its
/// device id before any data arrives (vauchi/private#706).
// @internal
#[test]
fn a_scanner_connecting_after_the_first_does_not_take_its_link() {
    let mut alice = displayer();
    let bob_offer = key_offer(&mut scanner_of_the_displayed_code(1, "bob"), "bob");
    alice.on_connected("bob", BleLinkDirection::Inbound, 0);
    alice.on_connected("mallory", BleLinkDirection::Inbound, 0);

    let answers_to_bob = key_acks_for(&mut alice, "bob", &bob_offer);

    assert_eq!(answers_to_bob, 1);
}

/// A client that connected earlier but never offered must not be able to
/// write a commitment-sized value into the exchange the displayer accepted.
// @internal
#[test]
fn a_connected_client_that_never_offered_cannot_write_into_the_accepted_exchange() {
    let mut alice = displayer();
    let mut bob = scanner_of_the_displayed_code(1, "bob");
    alice.on_connected("bob", BleLinkDirection::Inbound, 0);
    alice.on_connected("mallory", BleLinkDirection::Inbound, 0);
    let bob_offer = key_offer(&mut bob, "bob");
    let (_event, phase2) = alice.on_data_received(
        "bob",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &bob_offer,
        0,
    );
    let mut phase3 = Vec::new();
    for ack in writes_on(&phase2, CHAR_HANDSHAKE_NOTIFY) {
        phase3.extend(
            bob.on_data_received(
                "bob",
                BleLinkDirection::Outbound,
                CHAR_HANDSHAKE_NOTIFY,
                &ack,
                0,
            )
            .1,
        );
    }
    for chunk in writes_on(&phase2, CHAR_DATA_NOTIFY) {
        phase3.extend(
            bob.on_data_received(
                "bob",
                BleLinkDirection::Outbound,
                CHAR_DATA_NOTIFY,
                &chunk,
                0,
            )
            .1,
        );
    }
    alice.on_data_received(
        "mallory",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &[9u8; 32],
        0,
    );
    let commitment = writes_on(&phase3, CHAR_HANDSHAKE_WRITE)
        .pop()
        .expect("bob sends his commitment");
    alice.on_data_received(
        "bob",
        BleLinkDirection::Inbound,
        CHAR_HANDSHAKE_WRITE,
        &commitment,
        0,
    );
    let mut last_event = BleMachineEvent::None;
    for chunk in writes_on(&phase3, CHAR_DATA_WRITE) {
        last_event = alice
            .on_data_received("bob", BleLinkDirection::Inbound, CHAR_DATA_WRITE, &chunk, 0)
            .0;
    }

    assert!(
        matches!(last_event, BleMachineEvent::Completed(_)),
        "{last_event:?}"
    );
}

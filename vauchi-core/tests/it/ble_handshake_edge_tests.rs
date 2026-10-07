// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE handshake edges: the key-ack wire size, the 60-second offer and ack
//! windows, the session key both sides end with, and a reset starting a
//! fresh offer (vauchi/private#522).

use vauchi_core::ExchangeError;
use vauchi_core::exchange::{
    BleCardPayload, BleHandshakeSession, BleHandshakeState, KEY_ACK_SIZE, X3DHKeyPair,
};
use vauchi_core::identity::Identity;

const T0: u64 = 1_700_000_000;
const EXPIRY_SECS: u64 = 60;

fn card(identity: &Identity, name: &str) -> BleCardPayload {
    BleCardPayload::new(
        *identity.signing_public_key(),
        name.to_string(),
        *X3DHKeyPair::generate().public_key(),
        vec![("email".into(), "test@example.com".into())],
        None,
    )
}

fn pair() -> (BleHandshakeSession, BleHandshakeSession, Identity) {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let initiator = BleHandshakeSession::new_initiator(&alice, card(&alice, "Alice"), T0);
    let responder = BleHandshakeSession::new_responder(&bob, card(&bob, "Bob"), T0);
    (initiator, responder, alice)
}

// @internal
#[test]
fn a_key_ack_is_153_bytes_and_a_shorter_one_is_refused() {
    let (mut alice, mut bob, _) = pair();
    let offer = alice.create_key_offer().unwrap();
    let (ack, card) = bob.process_key_offer(&offer, T0).unwrap();

    assert_eq!((ack.len(), KEY_ACK_SIZE), (153, 153));
    assert!(
        alice
            .process_key_ack(&ack[..ack.len() - 1], &card, T0)
            .is_err()
    );
}

// @internal
#[test]
fn an_offer_is_accepted_for_exactly_sixty_seconds() {
    let (mut alice, mut bob, _) = pair();
    let offer = alice.create_key_offer().unwrap();
    assert!(bob.process_key_offer(&offer, T0 + EXPIRY_SECS).is_ok());

    let (mut alice, mut bob, _) = pair();
    let offer = alice.create_key_offer().unwrap();
    assert!(matches!(
        bob.process_key_offer(&offer, T0 + EXPIRY_SECS + 1),
        Err(ExchangeError::BleExpired)
    ));
}

// @internal
#[test]
fn an_ack_long_after_the_offer_window_is_refused() {
    let (mut alice, mut bob, _) = pair();
    let offer = alice.create_key_offer().unwrap();
    let (ack, card) = bob.process_key_offer(&offer, T0).unwrap();

    assert!(matches!(
        alice.process_key_ack(&ack, &card, T0 + EXPIRY_SECS + 30),
        Err(ExchangeError::BleExpired)
    ));
}

// @internal
#[test]
fn both_sides_hold_the_same_session_key_after_the_ack() {
    let (mut alice, mut bob, alice_identity) = pair();
    assert_eq!(
        alice.our_identity_key(),
        alice_identity.signing_public_key()
    );
    let offer = alice.create_key_offer().unwrap();
    let (ack, card) = bob.process_key_offer(&offer, T0).unwrap();

    alice.process_key_ack(&ack, &card, T0).unwrap();

    let alice_key = alice.session_key().expect("initiator session key");
    let bob_key = bob.session_key().expect("responder session key");
    assert_eq!(alice_key.as_bytes(), bob_key.as_bytes());
}

// @internal
#[test]
fn a_reset_returns_to_idle_with_fresh_ephemeral_keys() {
    let (mut alice, _, _) = pair();
    let first = alice.create_key_offer().unwrap();
    assert!(alice.create_key_offer().is_err(), "one offer per session");

    alice.reset(T0 + 5);

    assert!(matches!(alice.state(), BleHandshakeState::Idle));
    let second = alice.create_key_offer().unwrap();
    assert_ne!(first, second);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! BLE handshake edges: the key-ack wire size, the 60-second offer and ack
//! windows, the session key both sides end with, and a reset starting a
//! fresh offer (vauchi/private#522).

use vauchi_core::ExchangeError;
use vauchi_core::exchange::{BleCardPayload, BleHandshakeSession, BleHandshakeState, KEY_ACK_SIZE};
use vauchi_core::identity::Identity;

const T0: u64 = 1_700_000_000;
const EXPIRY_SECS: u64 = 60;

fn card(identity: &Identity, name: &str) -> BleCardPayload {
    BleCardPayload::new(
        *identity.signing_public_key(),
        name.to_string(),
        *identity.x3dh_keypair().public_key(),
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

// The ack carries the responder's own timestamp; it is accepted while no
// more than 60 seconds old, independently of the initiator's session age.
// @internal
#[test]
fn an_ack_is_fresh_for_sixty_seconds_after_the_responder_stamped_it() {
    let ack_age = |age: u64| {
        let alice_id = Identity::create("Alice", 0);
        let bob_id = Identity::create("Bob", 0);
        let mut alice =
            BleHandshakeSession::new_initiator(&alice_id, card(&alice_id, "Alice"), T0 + age);
        let mut bob = BleHandshakeSession::new_responder(&bob_id, card(&bob_id, "Bob"), T0);
        let offer = alice.create_key_offer().unwrap();
        let (ack, card) = bob.process_key_offer(&offer, T0 + age).unwrap();
        alice.process_key_ack(&ack, &card, T0 + age)
    };

    assert!(ack_age(EXPIRY_SECS).is_ok());
    assert!(matches!(
        ack_age(EXPIRY_SECS + 1),
        Err(ExchangeError::BleExpired)
    ));
}

/// The card a peer sends over Bluetooth must name the identity and
/// exchange keys the handshake used; otherwise the contact would be
/// stored under keys this exchange never proved (vauchi/private#941,
/// ship checklist SC-06).
// @internal
#[rstest::rstest]
#[case::initiator_receives_a_foreign_identity(true, true)]
#[case::initiator_receives_a_foreign_exchange_key(true, false)]
#[case::responder_receives_a_foreign_identity(false, true)]
#[case::responder_receives_a_foreign_exchange_key(false, false)]
fn ble_card_with_a_foreign_identity_key_is_refused(
    #[case] victim_is_initiator: bool,
    #[case] substitute_identity: bool,
) {
    let honest = Identity::create("Honest", 0);
    let malicious = Identity::create("Malicious", 0);
    let unrelated = Identity::create("Unrelated", 0);
    let honest_card = BleCardPayload::new(
        *honest.signing_public_key(),
        "Honest".into(),
        *honest.x3dh_keypair().public_key(),
        vec![],
        None,
    );
    let (claimed_identity, claimed_exchange) = if substitute_identity {
        (
            *unrelated.signing_public_key(),
            *malicious.x3dh_keypair().public_key(),
        )
    } else {
        (
            *malicious.signing_public_key(),
            *unrelated.x3dh_keypair().public_key(),
        )
    };
    let malicious_card = BleCardPayload::new(
        claimed_identity,
        "Forged card".into(),
        claimed_exchange,
        vec![],
        None,
    );
    let (mut initiator, mut responder) = if victim_is_initiator {
        (
            BleHandshakeSession::new_initiator(&honest, honest_card, T0),
            BleHandshakeSession::new_responder(&malicious, malicious_card, T0),
        )
    } else {
        (
            BleHandshakeSession::new_initiator(&malicious, malicious_card, T0),
            BleHandshakeSession::new_responder(&honest, honest_card, T0),
        )
    };
    let offer = initiator.create_key_offer().unwrap();
    let (ack, responder_card) = responder.process_key_offer(&offer, T0).unwrap();

    let error = if victim_is_initiator {
        initiator
            .process_key_ack(&ack, &responder_card, T0)
            .expect_err("a card naming keys the handshake did not use is refused")
    } else {
        let (commitment, initiator_card) = initiator
            .process_key_ack(&ack, &responder_card, T0)
            .unwrap();
        responder
            .process_committed_payload(&commitment, &initiator_card)
            .unwrap();
        responder
            .complete_exchange(&[])
            .expect_err("a card naming keys the handshake did not use is refused")
    };

    let expected_identity_refusal = substitute_identity;
    assert!(
        matches!(
            (expected_identity_refusal, &error),
            (true, ExchangeError::IdentityMismatch) | (false, ExchangeError::ExchangeKeyMismatch)
        ),
        "{error:?}"
    );
}

/// A Glance displayer requires the nonce its QR carried; the nonce is spent
/// by the first accepted offer (ADR-053, vauchi/private#892).
// @scenario: contact_exchange :: A Glance displayer accepts one KeyOffer per nonce
#[test]
fn a_second_key_offer_with_the_same_echo_is_refused() {
    let nonce = [0x5Au8; 16];
    let (mut alice, mut bob, _) = pair();
    alice.set_oob_nonce(nonce);
    bob.require_oob_nonce(nonce);
    let offer = alice.create_key_offer().unwrap();
    assert!(bob.process_key_offer(&offer, T0).is_ok());
    bob.reset(T0 + 1);
    let carol = Identity::create("Carol", 0);
    let mut carol_scanned_the_same_qr =
        BleHandshakeSession::new_initiator(&carol, card(&carol, "Carol"), T0 + 1);
    carol_scanned_the_same_qr.set_oob_nonce(nonce);
    let carols_offer = carol_scanned_the_same_qr.create_key_offer().unwrap();

    assert!(
        matches!(
            bob.process_key_offer(&offer, T0 + 1),
            Err(ExchangeError::OobNonceReplayed)
        ),
        "the same offer again, after the displayer reset, is a replay"
    );
    assert!(
        matches!(
            bob.process_key_offer(&carols_offer, T0 + 1),
            Err(ExchangeError::OobNonceReplayed)
        ),
        "a second scanner of the same QR echoes a spent nonce"
    );
    assert!(matches!(bob.state(), BleHandshakeState::Idle));
}

// @scenario: contact_exchange :: A Glance displayer accepts one KeyOffer per nonce
#[test]
fn a_responder_without_a_required_nonce_takes_a_fresh_offer_after_a_reset() {
    let (mut alice, mut bob, _) = pair();
    let first = alice.create_key_offer().unwrap();
    assert!(bob.process_key_offer(&first, T0).is_ok());
    bob.reset(T0 + 1);
    alice.reset(T0 + 1);

    let second = alice.create_key_offer().unwrap();

    assert!(bob.process_key_offer(&second, T0 + 1).is_ok());
}

/// The QR's 60-second window is the displayer's too: a Glance displayer
/// created with its QR refuses an offer once its own session is older than
/// sixty seconds, whatever `t_S` the connector chose (vauchi/private#948).
// @scenario: contact_exchange :: A Glance displayer closes its QR window after sixty seconds
#[test]
fn a_glance_displayer_refuses_an_offer_after_its_own_sixty_second_window() {
    let nonce = [0x5Au8; 16];
    let late = T0 + EXPIRY_SECS + 1;
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let mut displayer = BleHandshakeSession::new_responder(&bob, card(&bob, "Bob"), T0);
    displayer.require_oob_nonce(nonce);
    let mut scanner = BleHandshakeSession::new_initiator(&alice, card(&alice, "Alice"), late);
    scanner.set_oob_nonce(nonce);
    let fresh_looking_offer = scanner.create_key_offer().unwrap();

    assert!(
        matches!(
            displayer.process_key_offer(&fresh_looking_offer, late),
            Err(ExchangeError::BleExpired)
        ),
        "the connector's own fresh timestamp does not reopen the displayer's window"
    );

    let mut plain_responder = BleHandshakeSession::new_responder(&bob, card(&bob, "Bob"), T0);
    assert!(
        plain_responder
            .process_key_offer(&scanner.create_key_offer().unwrap(), late)
            .is_ok(),
        "a responder that required no nonce keeps waiting for an offer"
    );
}

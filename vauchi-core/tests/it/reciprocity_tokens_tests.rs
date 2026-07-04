// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the transport-agnostic reciprocity confirmation-token primitive
//! (`exchange::reciprocity_tokens`). QR, BLE, and multi-stage all derive tokens
//! through this function, so its cross-match property is what lets one shared
//! `ReciprocityConfirmer` verify a peer's token over any channel.

use proptest::prelude::*;
use vauchi_core::exchange::reciprocity_tokens::derive_confirmation_tokens;
use vauchi_core::exchange::{BleCardPayload, BleHandshakeSession, X3DHKeyPair};
use vauchi_core::identity::Identity;

fn make_test_identity() -> Identity {
    Identity::create("Test", 0)
}

fn make_test_card(identity: &Identity, name: &str) -> BleCardPayload {
    let exchange_keys = X3DHKeyPair::generate();
    BleCardPayload::new(
        *identity.signing_public_key(),
        name.to_string(),
        *exchange_keys.public_key(),
        vec![("email".into(), "test@example.com".into())],
        None,
    )
}

// The core invariant: from the SAME shared secret with role-swapped identity
// keys, A's own token equals what B expects of A (and vice versa).
// @internal
#[test]
fn tokens_cross_match_across_peers() {
    let secret = [7u8; 32];
    let alice = [1u8; 32];
    let bob = [2u8; 32];

    let (a_our, a_their) = derive_confirmation_tokens(&secret, &alice, &bob);
    let (b_our, b_their) = derive_confirmation_tokens(&secret, &bob, &alice);

    assert_eq!(
        *a_our, *b_their,
        "A's own token must equal what B expects of A"
    );
    assert_eq!(
        *b_our, *a_their,
        "B's own token must equal what A expects of B"
    );
}

// @internal
#[test]
fn tokens_are_self_asymmetric_and_secret_bound() {
    let secret = [7u8; 32];
    let (a_our, a_their) = derive_confirmation_tokens(&secret, &[1u8; 32], &[2u8; 32]);
    assert_ne!(
        *a_our, *a_their,
        "own vs expected token must differ (echo protection)"
    );

    let (a_our_other_secret, _) = derive_confirmation_tokens(&[8u8; 32], &[1u8; 32], &[2u8; 32]);
    assert_ne!(
        *a_our, *a_our_other_secret,
        "tokens must depend on the shared secret"
    );
}

proptest! {
    // CC-04: the cross-match property holds for arbitrary secrets + ids, and a
    // secret change always changes the token.
    // @internal
    #[test]
    fn cross_match_and_secret_binding_hold(
        secret in any::<[u8; 32]>(),
        other_secret in any::<[u8; 32]>(),
        a in any::<[u8; 32]>(),
        b in any::<[u8; 32]>(),
    ) {
        prop_assume!(a != b);
        prop_assume!(secret != other_secret);

        let (a_our, _) = derive_confirmation_tokens(&secret, &a, &b);
        let (_, b_their) = derive_confirmation_tokens(&secret, &b, &a);
        prop_assert_eq!(*a_our, *b_their, "cross-match must hold for any inputs");

        let (a_our_2, _) = derive_confirmation_tokens(&other_secret, &a, &b);
        prop_assert_ne!(*a_our, *a_our_2, "a different secret must change the token");
    }
}

// Session integration (BLE): after a full handshake both sides derive the pair
// from the agreed session key and it cross-matches — the property that lets the
// shared ReciprocityConfirmer confirm over the native BLE channel (relay-free).
// @scenario: ble_exchange :: both sides derive cross-matching reciprocity tokens
#[test]
fn ble_sessions_derive_cross_matching_confirmation_tokens() {
    let now = vauchi_core::clock::SystemClock::shared().unix_seconds();
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(&alice_id, alice_card, now);
    let mut bob = BleHandshakeSession::new_responder(&bob_id, bob_card, now);

    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_encrypted_card) = bob.process_key_offer(&offer, now).expect("process offer");
    alice
        .process_key_ack(&ack, &bob_encrypted_card, now)
        .expect("process ack");

    let alice_our = alice
        .our_confirmation_token()
        .expect("alice derived tokens");
    let alice_their = alice.expected_their_token().expect("alice derived tokens");
    let bob_our = bob.our_confirmation_token().expect("bob derived tokens");
    let bob_their = bob.expected_their_token().expect("bob derived tokens");

    assert_eq!(
        alice_our, bob_their,
        "Alice's own token must equal what Bob expects of Alice"
    );
    assert_eq!(
        bob_our, alice_their,
        "Bob's own token must equal what Alice expects of Bob"
    );
    assert_ne!(
        alice_our, alice_their,
        "own vs expected token must differ (echo protection)"
    );
}

/// Drive a full two-party BLE handshake; both sides end with a session key +
/// derived confirmation tokens, ready to exchange reciprocity acks.
fn ble_handshake_pair() -> (BleHandshakeSession, BleHandshakeSession) {
    let now = vauchi_core::clock::SystemClock::shared().unix_seconds();
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now);
    let mut bob = BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now);
    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_card) = bob.process_key_offer(&offer, now).expect("process offer");
    alice
        .process_key_ack(&ack, &bob_card, now)
        .expect("process ack");
    (alice, bob)
}

// CC-03: a valid post-persist reciprocity ack over the BLE channel confirms.
// @scenario: ble_exchange :: reciprocity ack confirms both directions
#[test]
fn ble_reciprocity_ack_confirms() {
    let (alice, bob) = ble_handshake_pair();

    let alice_ack = alice.build_reciprocity_ack().expect("alice builds ack");
    assert!(
        bob.process_reciprocity_ack(&alice_ack)
            .expect("bob processes"),
        "a matching ack must resolve Confirmed"
    );

    let bob_ack = bob.build_reciprocity_ack().expect("bob builds ack");
    assert!(
        alice
            .process_reciprocity_ack(&bob_ack)
            .expect("alice processes"),
        "a matching ack must resolve Confirmed (other direction)"
    );
}

// CC-14: a tampered ack must never confirm (the AEAD tag fails).
// @scenario: ble_exchange :: tampered reciprocity ack is rejected
#[test]
fn ble_reciprocity_ack_tampered_never_confirms() {
    let (alice, bob) = ble_handshake_pair();
    let mut ack = alice.build_reciprocity_ack().expect("ack");
    let last = ack.len() - 1;
    ack[last] ^= 0xFF;
    assert!(
        !matches!(bob.process_reciprocity_ack(&ack), Ok(true)),
        "a tampered ack must never confirm"
    );
}

// CC-14: an ack from a DIFFERENT session (fresh DH → different session key)
// must never confirm — cross-session replay is defeated.
// @scenario: ble_exchange :: foreign-session reciprocity ack is rejected
#[test]
fn ble_reciprocity_ack_foreign_session_never_confirms() {
    let (_alice, bob) = ble_handshake_pair();
    let (other_alice, _other_bob) = ble_handshake_pair();
    let foreign_ack = other_alice.build_reciprocity_ack().expect("ack");
    assert!(
        !matches!(bob.process_reciprocity_ack(&foreign_ack), Ok(true)),
        "an ack from another session must never confirm"
    );
}

// Confidentiality: the ack on the wire is AEAD ciphertext, not the raw token.
// @internal
#[test]
fn ble_reciprocity_ack_does_not_leak_raw_token() {
    let (alice, _bob) = ble_handshake_pair();
    let token = *alice.our_confirmation_token().expect("token");
    let ack = alice.build_reciprocity_ack().expect("ack");
    assert!(
        !ack.windows(token.len()).any(|w| w == token),
        "the raw confirmation token must not appear on the wire"
    );
}

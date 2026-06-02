// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the v2 link-mode card payload (ADR-050 Phase 2 T4) — the
//! signed bootstrap carrying the depositor's X3DH exchange key + relay
//! routing, plus version negotiation against the legacy v1 import payload.

use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_mode::{
    LinkCardPayload, derive_link_shared_key, parse_card_payload_versioned, serialize_card_payload,
    serialize_card_payload_v2,
};
use vauchi_core::{Contact, ContactCard, ExchangeTransport, SigningKeyPair};

fn sample() -> (SigningKeyPair, [u8; 32], [u8; 32], ContactCard) {
    let keypair = SigningKeyPair::generate();
    let identity = *keypair.public_key().as_bytes();
    let x3dh = *X3DHKeyPair::generate().public_key();
    let card = ContactCard::new("Alice");
    (keypair, identity, x3dh, card)
}

/// Re-prefix a mutated JSON body with the v2 version byte (2).
fn repack_v2(body: &serde_json::Value) -> Vec<u8> {
    let mut payload = vec![2u8];
    payload.extend_from_slice(&serde_json::to_vec(body).unwrap());
    payload
}

// @internal
#[test]
fn v2_round_trips_with_all_bootstrap_fields() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        Some([7u8; 32]),
        &card,
    );

    match parse_card_payload_versioned(&payload).expect("v2 parse") {
        LinkCardPayload::V2 {
            identity_pubkey,
            x3dh_pubkey,
            relay_url,
            relay_noise_pubkey,
            card,
        } => {
            assert_eq!(identity_pubkey, identity);
            assert_eq!(x3dh_pubkey, x3dh);
            assert_eq!(relay_url, "https://relay.example");
            assert_eq!(relay_noise_pubkey, Some([7u8; 32]));
            assert_eq!(card.display_name(), "Alice");
        }
        other => panic!("expected V2, got {other:?}"),
    }
}

// @internal
#[test]
fn v2_round_trips_without_relay_noise() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        None,
        &card,
    );
    match parse_card_payload_versioned(&payload).expect("v2 parse") {
        LinkCardPayload::V2 {
            relay_noise_pubkey, ..
        } => assert_eq!(relay_noise_pubkey, None),
        other => panic!("expected V2, got {other:?}"),
    }
}

// @internal
#[test]
fn v1_payload_parses_as_v1_via_versioned() {
    let (_, identity, _, card) = sample();
    let v1 = serialize_card_payload(&identity, &card);
    match parse_card_payload_versioned(&v1).expect("v1 parse") {
        LinkCardPayload::V1 {
            identity_pubkey,
            card,
        } => {
            assert_eq!(identity_pubkey, identity);
            assert_eq!(card.display_name(), "Alice");
        }
        other => panic!("expected V1, got {other:?}"),
    }
}

// @internal
#[test]
fn tampered_relay_url_fails_signature() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        None,
        &card,
    );
    let mut body: serde_json::Value = serde_json::from_slice(&payload[1..]).unwrap();
    body["relay_url"] = serde_json::json!("https://evil.example");

    let err = parse_card_payload_versioned(&repack_v2(&body))
        .expect_err("a tampered relay_url must fail the bootstrap signature");
    assert!(
        format!("{err}").contains("signature"),
        "expected a signature failure, got {err}"
    );
}

// @internal
#[test]
fn tampered_x3dh_key_fails_signature() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        None,
        &card,
    );
    let mut body: serde_json::Value = serde_json::from_slice(&payload[1..]).unwrap();
    // Flip the exchange key — a MITM swapping in its own would be caught.
    body["x3dh_pubkey"] = serde_json::to_value([9u8; 32]).unwrap();

    let err = parse_card_payload_versioned(&repack_v2(&body))
        .expect_err("a swapped X3DH key must fail the bootstrap signature");
    assert!(format!("{err}").contains("signature"), "got {err}");
}

// @internal
#[test]
fn substituted_identity_key_fails_signature() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        None,
        &card,
    );
    let other_identity = *SigningKeyPair::generate().public_key().as_bytes();
    let mut body: serde_json::Value = serde_json::from_slice(&payload[1..]).unwrap();
    body["identity_pubkey"] = serde_json::to_value(other_identity).unwrap();

    let err = parse_card_payload_versioned(&repack_v2(&body))
        .expect_err("verifying against a substituted identity key must fail");
    assert!(format!("{err}").contains("signature"), "got {err}");
}

// @internal
#[test]
fn wrong_length_signature_rejected() {
    let (keypair, identity, x3dh, card) = sample();
    let payload = serialize_card_payload_v2(
        &identity,
        &keypair,
        &x3dh,
        "https://relay.example",
        None,
        &card,
    );
    let mut body: serde_json::Value = serde_json::from_slice(&payload[1..]).unwrap();
    body["signature"] = serde_json::json!([1, 2, 3]);

    let err =
        parse_card_payload_versioned(&repack_v2(&body)).expect_err("short signature rejected");
    assert!(format!("{err}").contains("64 bytes"), "got {err}");
}

// @internal
#[test]
fn unknown_version_and_empty_payloads_rejected() {
    assert!(parse_card_payload_versioned(&[]).is_err());
    assert!(parse_card_payload_versioned(&[9, 1, 2, 3]).is_err());
}

// ── T5a: symmetric key agreement + link-exchange contact ─────────────

// @internal
#[test]
fn link_shared_key_is_symmetric() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_view = derive_link_shared_key(&alice, bob.public_key()).expect("alice derives");
    let bob_view = derive_link_shared_key(&bob, alice.public_key()).expect("bob derives");

    assert_eq!(
        alice_view.as_bytes(),
        bob_view.as_bytes(),
        "both sides must derive the identical link shared key (symmetric exchange)",
    );
}

// @internal
#[test]
fn link_shared_key_differs_per_peer() {
    let ours = X3DHKeyPair::generate();
    let peer_a = X3DHKeyPair::generate();
    let peer_b = X3DHKeyPair::generate();

    let ka = derive_link_shared_key(&ours, peer_a.public_key()).unwrap();
    let kb = derive_link_shared_key(&ours, peer_b.public_key()).unwrap();

    assert_ne!(ka.as_bytes(), kb.as_bytes());
}

// @internal
#[test]
fn from_link_exchange_stamps_link_transport_and_relay() {
    let ours = X3DHKeyPair::generate();
    let peer = X3DHKeyPair::generate();
    let shared = derive_link_shared_key(&ours, peer.public_key()).unwrap();
    let peer_identity = *SigningKeyPair::generate().public_key().as_bytes();

    let contact = Contact::from_link_exchange(
        peer_identity,
        ContactCard::new("Bob"),
        shared,
        Some("https://relay.example".to_string()),
        42,
    );

    // Link is an Exchange (counts in exchange_method_breakdown), not an
    // Import — that is the whole point of ADR-050.
    assert_eq!(
        contact.exchange_transport(),
        Some(ExchangeTransport::Link),
        "a link exchange must stamp ExchangeTransport::Link",
    );
    assert_eq!(contact.display_name(), "Bob");
}

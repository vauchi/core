// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for the link-mode card payload (ADR-050 Phase 2 T4) — the signed
//! bootstrap carrying the depositor's card, X3DH exchange key and relay
//! routing. The signature covers the whole body and the exchange it was
//! made for (#942); retired versions are refused.

use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_mode::{
    LinkCardPayload, LinkExchangeBinding, derive_link_shared_key, parse_link_card_payload,
    serialize_link_card_payload,
};
use vauchi_core::{Contact, ContactCard, ExchangeTransport, SigningKeyPair};

const EXCHANGE: LinkExchangeBinding = LinkExchangeBinding {
    nonce: [0x11; 32],
    initiator_epk: [0x22; 32],
    responder_epk: [0x33; 32],
};

fn sample() -> (SigningKeyPair, [u8; 32], [u8; 32], ContactCard) {
    let keypair = SigningKeyPair::generate();
    let identity = *keypair.public_key().as_bytes();
    let x3dh = *X3DHKeyPair::generate().public_key();
    let card = ContactCard::new("Alice");
    (keypair, identity, x3dh, card)
}

fn sample_payload() -> (Vec<u8>, [u8; 32], [u8; 32]) {
    let (keypair, identity, x3dh, card) = sample();
    let payload =
        serialize_link_card_payload(&keypair, &x3dh, "https://relay.example", &card, &EXCHANGE);
    (payload, identity, x3dh)
}

/// The payload's JSON body, after `[version][signature: 64]`.
fn body_of(payload: &[u8]) -> serde_json::Value {
    serde_json::from_slice(&payload[65..]).unwrap()
}

/// Reassemble a payload from its original version byte and signature and a
/// mutated body.
fn repack(payload: &[u8], body: &serde_json::Value) -> Vec<u8> {
    [&payload[..65], &serde_json::to_vec(body).unwrap()].concat()
}

// @internal
#[test]
fn payload_round_trips_with_all_bootstrap_fields() {
    let (payload, identity, x3dh) = sample_payload();

    let LinkCardPayload {
        identity_pubkey,
        x3dh_pubkey,
        relay_url,
        card,
    } = parse_link_card_payload(&payload, &EXCHANGE).expect("payload parses");

    assert_eq!(payload[0], 3);
    assert_eq!(identity_pubkey, identity);
    assert_eq!(x3dh_pubkey, x3dh);
    assert_eq!(relay_url, "https://relay.example");
    assert_eq!(card.display_name(), "Alice");
}

// @internal
#[test]
fn retired_payload_versions_are_refused() {
    let (_, identity, x3dh, card) = sample();
    let v1 = [&[1u8][..], &identity, &serde_json::to_vec(&card).unwrap()].concat();
    let v2_body = serde_json::json!({
        "identity_pubkey": identity, "x3dh_pubkey": x3dh,
        "relay_url": "https://relay.example", "signature": vec![0u8; 64], "card": card,
    });
    let v2 = [&[2u8][..], &serde_json::to_vec(&v2_body).unwrap()].concat();

    for retired in [v1, v2] {
        let err = parse_link_card_payload(&retired, &EXCHANGE)
            .expect_err("v1 (#574) and v2 (#942) are retired");
        assert!(
            err.to_string().contains("version"),
            "unexpected error: {err}"
        );
    }
}

// @internal
#[test]
fn every_tampered_body_field_fails_the_signature() {
    let (payload, _, _) = sample_payload();
    let other_identity = *SigningKeyPair::generate().public_key().as_bytes();
    let tamperings: [(&str, serde_json::Value); 4] = [
        ("relay_url", serde_json::json!("https://evil.example")),
        // A MITM swapping in its own exchange key would be caught.
        ("x3dh_pubkey", serde_json::to_value([9u8; 32]).unwrap()),
        (
            "identity_pubkey",
            serde_json::to_value(other_identity).unwrap(),
        ),
        (
            "card",
            serde_json::to_value(ContactCard::new("Mallory")).unwrap(),
        ),
    ];

    for (field, value) in tamperings {
        let mut body = body_of(&payload);
        body[field] = value;
        let err = parse_link_card_payload(&repack(&payload, &body), &EXCHANGE)
            .expect_err("a tampered field must fail the bootstrap signature");
        assert!(
            err.to_string().contains("signature"),
            "{field}: expected a signature failure, got {err}"
        );
    }
}

// @internal
#[test]
fn a_flipped_signature_bit_fails_the_signature() {
    let (mut payload, _, _) = sample_payload();
    payload[1] ^= 1;

    let err = parse_link_card_payload(&payload, &EXCHANGE).expect_err("bad signature");
    assert!(err.to_string().contains("signature"), "got {err}");
}

// @internal
#[test]
fn a_payload_shorter_than_its_signature_is_refused() {
    let (payload, _, _) = sample_payload();

    let err = parse_link_card_payload(&payload[..40], &EXCHANGE).expect_err("truncated");
    assert!(err.to_string().contains("signature"), "got {err}");
}

// @internal
#[test]
fn unknown_version_and_empty_payloads_rejected() {
    assert!(parse_link_card_payload(&[], &EXCHANGE).is_err());
    assert!(parse_link_card_payload(&[9, 1, 2, 3], &EXCHANGE).is_err());
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

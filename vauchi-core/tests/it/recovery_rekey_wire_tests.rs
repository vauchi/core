// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rekey offer and answer after a social recovery (vauchi/private#796
//! step 4a; `recovery_acceptance.spthy`, rules Alice_publishes,
//! John_answers and Alice_completes). The new identity signs a fresh
//! exchange key; the contact answers with its own, signing with its
//! identity key and naming the offer's key; both derive the new shared
//! key from the exchange and the two identities.

use proptest::prelude::*;
use vauchi_core::Identity;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::recovery::rekey::{
    MAX_PROOF_BYTES, RekeyAnswer, RekeyError, RekeyOffer, rekey_shared_key,
};

const PROOF: &[u8] = b"a recovery proof's bytes";

struct Parties {
    alice: Identity,
    john: Identity,
    alice_exchange: X3DHKeyPair,
    john_exchange: X3DHKeyPair,
}

fn parties() -> Parties {
    Parties {
        alice: Identity::create("Alice, new phone", 0),
        john: Identity::create("John", 0),
        alice_exchange: X3DHKeyPair::generate(),
        john_exchange: X3DHKeyPair::generate(),
    }
}

fn offer(p: &Parties) -> RekeyOffer {
    RekeyOffer::create(&p.alice, *p.alice_exchange.public_key(), PROOF.to_vec()).unwrap()
}

fn answer(p: &Parties) -> RekeyAnswer {
    RekeyAnswer::create(
        &p.john,
        *p.john_exchange.public_key(),
        *p.alice_exchange.public_key(),
    )
}

// @internal
#[test]
fn an_offer_round_trips_and_verifies_under_the_new_key() {
    let p = parties();

    let decoded = RekeyOffer::decode(&offer(&p).encode()).unwrap();

    assert_eq!(decoded.new_pk(), p.alice.signing_public_key());
    assert_eq!(decoded.exchange_key(), p.alice_exchange.public_key());
    assert_eq!(decoded.proof(), PROOF);
    assert!(decoded.verify());
}

// @internal
#[test]
fn an_answer_verifies_only_for_the_contact_key_and_the_offer_it_names() {
    let p = parties();
    let decoded = RekeyAnswer::decode(&answer(&p).encode()).unwrap();

    assert!(decoded.verify(p.john.signing_public_key(), p.alice_exchange.public_key()));
    assert!(!decoded.verify(
        Identity::create("Mallory", 0).signing_public_key(),
        p.alice_exchange.public_key()
    ));
    assert!(!decoded.verify(
        p.john.signing_public_key(),
        X3DHKeyPair::generate().public_key()
    ));
}

// @internal
#[test]
fn both_sides_derive_the_same_shared_key_and_a_third_does_not() {
    let p = parties();
    let alice_pk = p.alice.signing_public_key();
    let john_pk = p.john.signing_public_key();

    let at_alice = rekey_shared_key(
        &p.alice_exchange,
        p.john_exchange.public_key(),
        alice_pk,
        john_pk,
    )
    .unwrap();
    let at_john = rekey_shared_key(
        &p.john_exchange,
        p.alice_exchange.public_key(),
        alice_pk,
        john_pk,
    )
    .unwrap();
    let stranger = rekey_shared_key(
        &X3DHKeyPair::generate(),
        p.alice_exchange.public_key(),
        alice_pk,
        john_pk,
    )
    .unwrap();
    let other_identity = rekey_shared_key(
        &p.john_exchange,
        p.alice_exchange.public_key(),
        alice_pk,
        Identity::create("Mallory", 0).signing_public_key(),
    )
    .unwrap();

    assert_eq!(at_alice.as_bytes(), at_john.as_bytes());
    assert_ne!(at_alice.as_bytes(), stranger.as_bytes());
    assert_ne!(at_alice.as_bytes(), other_identity.as_bytes());
}

// @internal
#[test]
fn an_offer_signed_by_another_key_does_not_verify() {
    let p = parties();
    let mut bytes = offer(&p).encode();
    let forged = RekeyOffer::create(
        &Identity::create("Thief", 0),
        *p.alice_exchange.public_key(),
        PROOF.to_vec(),
    )
    .unwrap()
    .encode();
    // Alice's key with the thief's signature.
    let signature_at = 4 + 32 + 32;
    bytes[signature_at..signature_at + 64]
        .copy_from_slice(&forged[signature_at..signature_at + 64]);

    assert!(!RekeyOffer::decode(&bytes).unwrap().verify());
}

// @internal
#[test]
fn an_offer_does_not_verify_as_an_answer_nor_an_answer_as_an_offer() {
    let p = parties();

    assert_eq!(
        RekeyAnswer::decode(&offer(&p).encode()).err(),
        Some(RekeyError::Malformed)
    );
    assert_eq!(
        RekeyOffer::decode(&answer(&p).encode()).err(),
        Some(RekeyError::Malformed)
    );
}

// @internal
#[test]
fn malformed_offers_are_refused() {
    let p = parties();
    let good = offer(&p).encode();
    let mut trailing = good.clone();
    trailing.push(0);
    let mut lying_length = good.clone();
    let length_at = 4 + 32 + 32 + 64;
    lying_length[length_at..length_at + 4].copy_from_slice(&u32::MAX.to_be_bytes());

    for (name, bytes) in [
        ("empty", Vec::new()),
        ("magic only", good[..4].to_vec()),
        ("truncated", good[..good.len() - 1].to_vec()),
        ("trailing byte", trailing),
        ("length past the end", lying_length),
    ] {
        assert_eq!(
            RekeyOffer::decode(&bytes).err(),
            Some(RekeyError::Malformed),
            "{name}"
        );
    }
}

// @internal
#[test]
fn an_empty_or_oversized_proof_is_refused() {
    let p = parties();
    let key = *p.alice_exchange.public_key();

    assert_eq!(
        RekeyOffer::create(&p.alice, key, Vec::new()).err(),
        Some(RekeyError::Malformed)
    );
    assert_eq!(
        RekeyOffer::create(&p.alice, key, vec![0; MAX_PROOF_BYTES + 1]).err(),
        Some(RekeyError::Malformed)
    );
    assert!(RekeyOffer::create(&p.alice, key, vec![0; MAX_PROOF_BYTES]).is_ok());
}

proptest! {
    /// Any single flipped byte of the signed part breaks the offer.
    // @internal
    #[test]
    fn a_flipped_byte_of_the_signed_offer_never_verifies(at in 4usize..(4 + 32 + 32 + 64), bit in 0u8..8) {
        let p = parties();
        let mut bytes = offer(&p).encode();
        bytes[at] ^= 1 << bit;

        let still_verifies = RekeyOffer::decode(&bytes).is_ok_and(|offer| offer.verify());
        prop_assert!(!still_verifies);
    }

    /// Any single flipped byte of an answer breaks it.
    // @internal
    #[test]
    fn a_flipped_byte_of_an_answer_never_verifies(at in 4usize..(4 + 32 + 32 + 32 + 64), bit in 0u8..8) {
        let p = parties();
        let mut bytes = answer(&p).encode();
        bytes[at] ^= 1 << bit;

        let still_verifies = RekeyAnswer::decode(&bytes).is_ok_and(|answer| {
            answer.verify(p.john.signing_public_key(), p.alice_exchange.public_key())
        });
        prop_assert!(!still_verifies);
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The signed multi-stage payload (vauchi/private#1217): a peer's payload
//! opens only for the session id, ephemeral and relay URL its INIT showed,
//! under the identity it names, with a card inside the card limits.

use proptest::prelude::*;
use vauchi_core::exchange::multistage::payload::{PayloadError, build, open};
use vauchi_core::{ContactCard, ContactField, FieldType, Identity};

const SID: [u8; 16] = [0x21; 16];
const EPH: [u8; 32] = [0x43; 32];
const RELAY: Option<&str> = Some("https://relay.example");

fn card(name: &str) -> ContactCard {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "b@b.example",
        0,
    ))
    .unwrap();
    card
}

fn bob() -> Identity {
    Identity::create("Bob", 0)
}

const ALICE_PK: [u8; 32] = [0xAA; 32];

// @internal
#[test]
fn a_payload_opens_for_the_session_it_was_signed_for() {
    let bob = bob();
    let payload = build(&bob, &SID, &EPH, RELAY, &card("Bob"));

    let peer = open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap();

    assert_eq!(&peer.identity_pk, bob.signing_public_key());
    assert_eq!(peer.card.display_name(), "Bob");
    assert_eq!(peer.card.fields()[0].value(), "b@b.example");
}

// @internal
#[test]
fn a_payload_opened_against_another_session_id_ephemeral_or_relay_fails() {
    let payload = build(&bob(), &SID, &EPH, RELAY, &card("Bob"));

    assert_eq!(
        open(&payload, &[0x22; 16], &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Signature
    );
    assert_eq!(
        open(&payload, &SID, &[0x44; 32], RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Signature
    );
    assert_eq!(
        open(
            &payload,
            &SID,
            &EPH,
            Some("https://other.example"),
            &ALICE_PK
        )
        .unwrap_err(),
        PayloadError::Signature,
        "the signed relay URL must be the INIT's"
    );
    assert_eq!(
        open(&payload, &SID, &EPH, None, &ALICE_PK).unwrap_err(),
        PayloadError::Signature
    );
}

// @internal
#[test]
fn a_payload_naming_another_identity_fails() {
    let mut payload = build(&bob(), &SID, &EPH, RELAY, &card("Bob"));
    let carol = Identity::create("Carol", 0);
    payload[1..33].copy_from_slice(carol.signing_public_key());

    assert_eq!(
        open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Signature
    );
}

// @internal
#[test]
fn a_payload_whose_card_changed_after_signing_fails() {
    let mut payload = build(&bob(), &SID, &EPH, RELAY, &card("Bob"));
    payload.truncate(1 + 32 + 64);
    payload.extend_from_slice(&serde_json::to_vec(&card("Mallory")).unwrap());

    assert_eq!(
        open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Signature
    );
}

// @internal
#[test]
fn a_payload_carrying_our_own_identity_fails_even_when_signed() {
    let alice = Identity::create("Alice", 0);
    let payload = build(&alice, &SID, &EPH, RELAY, &card("Alice"));

    assert_eq!(
        open(&payload, &SID, &EPH, RELAY, alice.signing_public_key()).unwrap_err(),
        PayloadError::OwnIdentity
    );
}

// @internal
#[test]
fn a_payload_of_another_version_or_cut_short_fails() {
    let mut payload = build(&bob(), &SID, &EPH, RELAY, &card("Bob"));

    assert_eq!(
        open(&payload[..96], &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Truncated
    );
    assert_eq!(
        open(&[], &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Version
    );
    payload[0] = 1;
    assert_eq!(
        open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::Version
    );
}

// @internal
#[test]
fn a_signed_card_over_the_size_limit_fails_before_its_signature_is_checked() {
    let mut payload = vec![2u8];
    payload.extend_from_slice(&[0u8; 96]);
    payload.extend(std::iter::repeat_n(
        b' ',
        vauchi_core::card::MAX_CARD_SIZE_BYTES + 1,
    ));

    assert_eq!(
        open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
        PayloadError::TooLarge
    );
}

/// A payload signed by `identity` over `card_json` as it stands, valid or
/// not: the signed message as the spec lays it out.
fn signed_over(identity: &Identity, card_json: &[u8]) -> Vec<u8> {
    let relay = RELAY.unwrap().as_bytes();
    let pk = identity.signing_public_key();
    let mut message = b"vauchi-multistage-payload-v2".to_vec();
    message.extend_from_slice(&SID);
    message.extend_from_slice(&EPH);
    message.extend_from_slice(pk);
    message.extend_from_slice(&u16::try_from(relay.len()).unwrap().to_be_bytes());
    message.extend_from_slice(relay);
    message.extend_from_slice(card_json);
    let mut payload = vec![2u8];
    payload.extend_from_slice(pk);
    payload.extend_from_slice(identity.sign(&message).as_bytes());
    payload.extend_from_slice(card_json);
    payload
}

// @internal
#[test]
fn the_signed_message_is_the_one_the_spec_lays_out() {
    let bob = bob();
    let card_json = serde_json::to_vec(&card("Bob")).unwrap();

    assert!(open(&signed_over(&bob, &card_json), &SID, &EPH, RELAY, &ALICE_PK).is_ok());
}

// @internal
#[test]
fn a_signed_payload_whose_card_is_not_a_card_or_has_an_invalid_field_fails() {
    let bob = bob();
    let invalid_email = String::from_utf8(serde_json::to_vec(&card("Bob")).unwrap())
        .unwrap()
        .replace("b@b.example", "not an email");

    for card_json in [b"{not json".to_vec(), invalid_email.into_bytes()] {
        assert_eq!(
            open(&signed_over(&bob, &card_json), &SID, &EPH, RELAY, &ALICE_PK).unwrap_err(),
            PayloadError::Card
        );
    }
}

proptest! {
    // @internal
    #[test]
    fn any_card_round_trips_and_any_flipped_byte_is_refused(
        name in "[A-Za-z]{1,40}",
        flip in 0usize..400,
        bit in 0u8..8,
    ) {
        let bob = bob();
        let payload = build(&bob, &SID, &EPH, RELAY, &card(&name));
        let peer = open(&payload, &SID, &EPH, RELAY, &ALICE_PK).unwrap();
        prop_assert_eq!(peer.card.display_name(), name.as_str());

        let mut tampered = payload.clone();
        let at = flip % tampered.len();
        tampered[at] ^= 1 << bit;
        prop_assert!(open(&tampered, &SID, &EPH, RELAY, &ALICE_PK).is_err());
    }
}

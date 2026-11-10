// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact applies a card delta only when the device it names as signer
//! is one the contact pinned, is active in the registry it holds, and
//! signed the delta with that key (vauchi/private#970, SC-62). Everything
//! goes through the public receive path a relay delivery takes.

use crate::common::card_update::{recipient_pk, seal_delta};
use crate::common::device_sync::device_signed_broadcast;
use crate::common::per_device::{Party, tablet_entry, two_parties};
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::sync::delta::CardDelta;

fn alice_holds_bobs_registry(p: &Party) {
    let honest = device_signed_broadcast(&p.bob);
    p.push_to_alice(&honest).unwrap();
}

fn bobs_cards() -> (ContactCard, ContactCard) {
    (ContactCard::new("Bob"), ContactCard::new("Robert"))
}

/// A delta Bob computed and signed with the identity key only, for a test
/// to add or withhold a device signature.
fn identity_signed_delta(p: &Party, version: u32) -> CardDelta {
    let (old, new) = bobs_cards();
    let mut delta = CardDelta::compute(&old, &new, p.now());
    delta.set_version(version);
    delta.sign(
        p.bob.identity().unwrap(),
        &recipient_pk(&p.bob, &p.alice_at_bob),
    );
    delta
}

fn deliver(p: &Party, delta: &CardDelta) -> Result<ReceiveOutcome, CardUpdateError> {
    let blob = seal_delta(&p.bob, &p.alice_at_bob, delta);
    process_single_card_update(
        p.alice.identity().unwrap(),
        p.alice.storage(),
        &p.bob_at_alice,
        &blob,
    )
}

fn bobs_name_at_alice(p: &Party) -> String {
    p.alice
        .get_contact(&p.bob_at_alice)
        .unwrap()
        .unwrap()
        .display_name()
        .to_string()
}

fn refused_as_device_signature(result: Result<ReceiveOutcome, CardUpdateError>) {
    match result {
        Err(CardUpdateError::DeviceSignatureInvalid) => {}
        other => panic!("expected DeviceSignatureInvalid, got {other:?}"),
    }
}

fn refused_as_not_yet_known(result: Result<ReceiveOutcome, CardUpdateError>) {
    match result {
        Err(CardUpdateError::DeviceNotYetKnown) => {}
        other => panic!("expected DeviceNotYetKnown, got {other:?}"),
    }
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_delta_the_sending_device_signed_is_applied() {
    let p = two_parties();
    alice_holds_bobs_registry(&p);
    let (old, new) = bobs_cards();

    let payloads = p
        .bob
        .prepare_card_updates_for_contact(&p.alice_at_bob, &old, &new)
        .unwrap();
    assert_eq!(
        payloads.len(),
        1,
        "one legacy session, no peer devices known"
    );
    let outcome = process_single_card_update(
        p.alice.identity().unwrap(),
        p.alice.storage(),
        &p.bob_at_alice,
        &payloads[0].1,
    )
    .unwrap();

    assert!(matches!(outcome, ReceiveOutcome::CardDelta));
    assert_eq!(bobs_name_at_alice(&p), "Robert");
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_linked_device_cannot_forge_a_card_delta_for_another_device() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let with_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&with_tablet).unwrap();

    let mut phone_claims_tablet = identity_signed_delta(&p, 1);
    phone_claims_tablet.sign_by_device([7u8; 32], &phone_key);
    refused_as_device_signature(deliver(&p, &phone_claims_tablet));

    let mut tablet_claims_phone = identity_signed_delta(&p, 1);
    tablet_claims_phone.sign_by_device(phone_id, &tablet_key);
    refused_as_device_signature(deliver(&p, &tablet_claims_phone));

    assert_eq!(
        bobs_name_at_alice(&p),
        "Bob",
        "no forged delta reached the card"
    );
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_delta_with_no_device_signature_is_refused() {
    let p = two_parties();
    alice_holds_bobs_registry(&p);

    let result = deliver(&p, &identity_signed_delta(&p, 1));

    refused_as_device_signature(result);
    assert_eq!(bobs_name_at_alice(&p), "Bob");
}

/// A signer the contact has never seen is the one refusal a later
/// registry push can resolve, so it is reported apart from a bad
/// signature.
// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_delta_from_a_device_the_contact_never_pinned_waits_for_its_registry() {
    let p = two_parties();
    alice_holds_bobs_registry(&p);
    let stranger = SigningKeyPair::generate();
    let mut delta = identity_signed_delta(&p, 1);
    delta.sign_by_device([7u8; 32], &stranger);

    refused_as_not_yet_known(deliver(&p, &delta));
    assert_eq!(bobs_name_at_alice(&p), "Bob");
}

/// The same sealed blob, refused before the sender's registry named its
/// device, applies once the registry arrives: the refusal consumed no
/// ratchet state and the relay kept the blob.
// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_refused_delta_from_a_new_device_applies_once_its_registry_arrives() {
    let p = two_parties();
    alice_holds_bobs_registry(&p);
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let tablet_key = SigningKeyPair::generate();
    let mut delta = identity_signed_delta(&p, 1);
    delta.sign_by_device([7u8; 32], &tablet_key);
    let blob = seal_delta(&p.bob, &p.alice_at_bob, &delta);
    let deliver_blob = || {
        process_single_card_update(
            p.alice.identity().unwrap(),
            p.alice.storage(),
            &p.bob_at_alice,
            &blob,
        )
    };

    refused_as_not_yet_known(deliver_blob());
    assert_eq!(bobs_name_at_alice(&p), "Bob");

    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let with_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&with_tablet).unwrap();

    let outcome = deliver_blob().expect("the retried blob applies");
    assert!(matches!(outcome, ReceiveOutcome::CardDelta));
    assert_eq!(bobs_name_at_alice(&p), "Robert");
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_delta_from_a_removed_device_is_refused() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), identity_key)
        .unwrap();
    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let with_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&with_tablet).unwrap();
    registry
        .revoke_device(&[7u8; 32], identity_key, now)
        .unwrap();
    let without_tablet =
        RegistryBroadcast::signed_by_device(&registry, identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&without_tablet).unwrap();

    let mut delta = identity_signed_delta(&p, 1);
    delta.sign_by_device([7u8; 32], &tablet_key);

    refused_as_device_signature(deliver(&p, &delta));
    assert_eq!(bobs_name_at_alice(&p), "Bob");
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_device_signed_delta_round_trips_and_any_flipped_byte_fails() {
    let p = two_parties();
    let (phone_id, phone_key) = (p.bob_device_id(), p.bob_device_key());
    let mut delta = identity_signed_delta(&p, 1);
    delta.sign_by_device(phone_id, &phone_key);
    let sender_pk = *p.bob.identity().unwrap().signing_public_key();
    let recipient = recipient_pk(&p.bob, &p.alice_at_bob);
    let phone_pk = *phone_key.public_key().as_bytes();

    let json = serde_json::to_string(&delta).unwrap();
    let back: CardDelta = serde_json::from_str(&json).unwrap();
    assert!(back.verify(&sender_pk, &recipient));
    assert!(back.verify_device(&phone_id, &phone_pk));

    let mut tampered = back.clone();
    tampered.device_signature.as_mut().unwrap()[3] ^= 0x01;
    assert!(!tampered.verify_device(&phone_id, &phone_pk));
    let mut edited = back.clone();
    edited.changes.clear();
    assert!(
        !edited.verify(&sender_pk, &recipient),
        "the identity signature covers the changes; the device signature covers it"
    );
    let mut resigned = edited.clone();
    resigned.signature = [0u8; 64];
    assert!(!resigned.verify_device(&phone_id, &phone_pk));
    assert!(!back.verify_device(&[9u8; 32], &phone_pk));
    assert!(!back.verify_device(&phone_id, &[0u8; 32]));
}

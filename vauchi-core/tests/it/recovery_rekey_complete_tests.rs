// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Alice's new phone completes a rekey (vauchi/private#796 step 6;
//! `recovery_acceptance.spthy`, Alice_completes): it takes a contact's
//! answer only when the contact's key from the backup signed it and it
//! names the offer's exchange key, derives the shared key the contact
//! derived, and starts the new relationship as the ratchet's initiator.
//! The first message on it ends the contact's resends of the answer.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::Identity;
use vauchi_core::Vauchi;
use vauchi_core::api::RekeyAnswerTaken;
use vauchi_core::clock::FakeClock;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::recovery::rekey::{RekeyAnswer, RekeyOffer};

use crate::common::meeting::{deliver, signing_pk};
use crate::common::recovered_phone::{World, recovered};

/// Dave accepted the rekey Alice's restored phone offered; returns the
/// world, the offer and Dave's answer.
fn answered() -> (World, RekeyOffer, Vec<u8>) {
    let mut phone =
        Vauchi::in_memory_with_clock(Arc::new(FakeClock::new(SystemTime::now()))).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let offer = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();
    world.dave.vauchi().take_rekey_offer(&offer).unwrap();
    let answer = world.dave.vauchi().rekey_answer_deliveries().unwrap()[0]
        .blob
        .clone();
    (world, RekeyOffer::decode(&offer).unwrap(), answer)
}

// @internal
#[test]
fn the_phone_completes_with_daves_answer_and_both_hold_one_key() {
    let (world, _, answer) = answered();
    let phone_at_dave = hex::encode(signing_pk(&world.phone));

    let taken = world.phone.take_rekey_answer(&answer).unwrap();

    assert_eq!(
        taken,
        RekeyAnswerTaken::Completed {
            contact_id: world.dave_id.clone()
        }
    );
    let at_phone = world.phone.get_contact(&world.dave_id).unwrap().unwrap();
    let at_dave = world
        .dave
        .vauchi()
        .get_contact(&phone_at_dave)
        .unwrap()
        .unwrap();
    assert_eq!(
        at_phone.shared_key().unwrap().as_bytes(),
        at_dave.shared_key().unwrap().as_bytes()
    );
    assert!(
        world
            .phone
            .storage()
            .ratchets()
            .load_ratchet_state(&world.dave_id)
            .unwrap()
            .is_some()
    );
    assert!(
        !world
            .phone
            .rekey_pending_contacts()
            .unwrap()
            .contains(&world.dave_id)
    );
}

// @internal
#[test]
fn the_new_relationship_carries_alices_card_and_ends_daves_resends() {
    let (world, _, answer) = answered();
    let dave = world.dave.vauchi();
    let phone_at_dave = hex::encode(signing_pk(&world.phone));
    world.phone.take_rekey_answer(&answer).unwrap();

    world
        .phone
        .queue_initial_card_for_contact(&world.dave_id)
        .unwrap();
    let outcomes = deliver(&world.phone, &world.dave_id, dave, &phone_at_dave);

    assert!(!outcomes.is_empty(), "the phone's first message decrypts");
    world.dave_clock.advance(Duration::from_secs(2 * 86_400));
    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
}

// @internal
#[test]
fn an_answer_signed_by_another_key_or_naming_another_offer_is_dropped() {
    let (world, offer, answer) = answered();
    let mallory = Identity::create("Mallory", 0);
    let by_mallory = RekeyAnswer::create(
        &mallory,
        *X3DHKeyPair::generate().public_key(),
        *offer.exchange_key(),
    )
    .encode();
    let mut tampered = answer.clone();
    let offer_key_at = 4 + 32 + 32;
    tampered[offer_key_at] ^= 1;

    for bytes in [by_mallory, tampered, Vec::new(), b"VRK2".to_vec()] {
        assert_eq!(
            world.phone.take_rekey_answer(&bytes).unwrap(),
            RekeyAnswerTaken::Dropped
        );
    }
    assert!(
        world
            .phone
            .rekey_pending_contacts()
            .unwrap()
            .contains(&world.dave_id)
    );
}

// @internal
#[test]
fn an_answer_taken_again_changes_nothing() {
    let (world, _, answer) = answered();
    world.phone.take_rekey_answer(&answer).unwrap();
    let key = world
        .phone
        .get_contact(&world.dave_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .unwrap()
        .clone();

    assert_eq!(
        world.phone.take_rekey_answer(&answer).unwrap(),
        RekeyAnswerTaken::Dropped
    );
    assert_eq!(
        world
            .phone
            .get_contact(&world.dave_id)
            .unwrap()
            .unwrap()
            .shared_key()
            .unwrap()
            .as_bytes(),
        key.as_bytes()
    );
}

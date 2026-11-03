// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact takes in a rekey offer (vauchi/private#796 step 5a): it keeps
//! one only when the new identity signed it and the recovery proof it
//! carries opens for one of its contacts and names that same identity, and
//! lists it for review with the proof's verdict. Anything else is dropped.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::recovery::rekey::RekeyOffer;
use vauchi_core::{Identity, Storage, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};

/// Alice's phone, restored, and the offer it makes Dave.
fn offered() -> (World, Vec<u8>) {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let blob = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();
    (world, blob)
}

// @internal
#[test]
fn dave_keeps_an_offer_whose_proof_names_its_key_and_lists_it_for_review() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();

    let taken = dave.take_rekey_offer(&blob).unwrap();

    let alice_at_dave = world.dave.sharer_id().to_string();
    assert_eq!(
        taken,
        RekeyOfferTaken::Kept {
            contact_id: alice_at_dave.clone()
        }
    );
    let pending = dave.pending_rekey_offers().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].contact_id, alice_at_dave);
    assert_eq!(pending[0].new_pk, signing_pk(&world.phone));
}

// @internal
#[test]
fn an_offer_signed_by_an_identity_the_proof_does_not_name_is_dropped() {
    let (world, blob) = offered();
    let honest = RekeyOffer::decode(&blob).unwrap();
    let thief = Identity::create("Thief", 0);
    let forged = RekeyOffer::create(&thief, *honest.exchange_key(), honest.proof().to_vec())
        .unwrap()
        .encode();

    let taken = world.dave.vauchi().take_rekey_offer(&forged).unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
    assert!(
        world
            .dave
            .vauchi()
            .pending_rekey_offers()
            .unwrap()
            .is_empty()
    );
}

// @internal
#[test]
fn an_offer_for_someone_who_is_not_a_contact_is_dropped() {
    let (_world, blob) = offered();
    let stranger = create_vauchi_with_identity("Stranger");

    assert_eq!(
        stranger.take_rekey_offer(&blob).unwrap(),
        RekeyOfferTaken::Dropped
    );
    assert!(stranger.pending_rekey_offers().unwrap().is_empty());
}

// @internal
#[test]
fn garbled_or_tampered_offers_are_dropped() {
    let (world, blob) = offered();
    let mut tampered = blob.clone();
    tampered[4 + 32] ^= 1;

    for bytes in [Vec::new(), b"VRK1".to_vec(), tampered] {
        assert_eq!(
            world.dave.vauchi().take_rekey_offer(&bytes).unwrap(),
            RekeyOfferTaken::Dropped
        );
    }
    assert!(
        world
            .dave
            .vauchi()
            .pending_rekey_offers()
            .unwrap()
            .is_empty()
    );
}

// @internal
#[test]
fn an_offer_sent_again_is_listed_once() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();

    dave.take_rekey_offer(&blob).unwrap();
    dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(dave.pending_rekey_offers().unwrap().len(), 1);
}

// @internal
#[test]
fn a_kept_offer_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = Contact::from_exchange(
        *Identity::create("Alice", 0).signing_public_key(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    storage
        .recovery()
        .keep_rekey_offer(contact.id(), b"offer bytes")
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage.recovery().kept_rekey_offers().unwrap(),
        vec![(contact.id().to_string(), b"offer bytes".to_vec())]
    );
}

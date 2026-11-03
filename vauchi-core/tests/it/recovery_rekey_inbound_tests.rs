// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact takes in a rekey offer (vauchi/private#796). It accepts one
//! only with a recovery authorization, which only the policy guardians'
//! decision certificate will grant (P15, P18): until then every offer is
//! dropped, even one the contact's own contacts vouched for, since two
//! legitimate attempts could each gather vouchers (P11). With an
//! authorization the offer must still be signed by the identity it names,
//! that identity must be the authorized one, and the old key must be an
//! exchanged contact's; anything else is dropped without a trace.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::clock::FakeClock;
use vauchi_core::recovery::authorization::RecoveryAuthorization;
use vauchi_core::recovery::rekey::RekeyOffer;
use vauchi_core::{Identity, Vauchi};

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

fn authorization_for(world: &World, new_pk: [u8; 32]) -> RecoveryAuthorization {
    RecoveryAuthorization::assumed_for_tests(signing_pk(&world.alice), new_pk)
}

// @internal
#[test]
fn an_authorized_offer_is_accepted_without_asking() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();

    let taken = world.dave_takes_as_authorized(&blob);

    let new_id = hex::encode(signing_pk(&world.phone));
    assert_eq!(
        taken,
        RekeyOfferTaken::Accepted {
            contact_id: new_id.clone()
        }
    );
    assert!(dave.get_contact(&old_id).unwrap().is_none());
    assert!(dave.get_contact(&new_id).unwrap().is_some());
}

// @internal
#[test]
fn an_offer_daves_own_contacts_vouched_for_is_dropped_until_certificates_exist() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();

    let taken = dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(
        taken,
        RekeyOfferTaken::Dropped,
        "two legitimate attempts could each gather vouchers (P11, P18)"
    );
    assert!(dave.get_contact(&old_id).unwrap().is_some());
}

// @internal
#[test]
fn an_offer_signed_by_an_identity_the_authorization_does_not_name_is_dropped() {
    let (world, blob) = offered();
    let honest = RekeyOffer::decode(&blob).unwrap();
    let thief = Identity::create("Thief", 0);
    let forged = RekeyOffer::create(&thief, *honest.exchange_key(), honest.proof().to_vec())
        .unwrap()
        .encode();
    let authorization = authorization_for(&world, signing_pk(&world.phone));

    let taken = world
        .dave
        .vauchi()
        .take_rekey_offer_authorized(&forged, &authorization)
        .unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
    assert!(
        world
            .dave
            .vauchi()
            .get_contact(world.dave.sharer_id())
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn an_authorization_for_another_identity_takes_no_offer() {
    let (world, blob) = offered();
    let authorization = authorization_for(&world, [7; 32]);

    let taken = world
        .dave
        .vauchi()
        .take_rekey_offer_authorized(&blob, &authorization)
        .unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
}

// @internal
#[test]
fn an_authorized_offer_for_someone_who_is_not_a_contact_is_dropped() {
    let (world, blob) = offered();
    let stranger = create_vauchi_with_identity("Stranger");
    let authorization = authorization_for(&world, signing_pk(&world.phone));

    assert_eq!(
        stranger
            .take_rekey_offer_authorized(&blob, &authorization)
            .unwrap(),
        RekeyOfferTaken::Dropped
    );
}

// @internal
#[test]
fn garbled_or_tampered_offers_are_dropped_even_when_authorized() {
    let (world, blob) = offered();
    let mut tampered = blob.clone();
    tampered[4 + 32] ^= 1;
    let authorization = authorization_for(&world, signing_pk(&world.phone));

    for bytes in [Vec::new(), b"VRK1".to_vec(), tampered] {
        assert_eq!(
            world
                .dave
                .vauchi()
                .take_rekey_offer_authorized(&bytes, &authorization)
                .unwrap(),
            RekeyOfferTaken::Dropped
        );
    }
    assert!(
        world
            .dave
            .vauchi()
            .get_contact(world.dave.sharer_id())
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn an_offer_sent_again_after_the_rekey_changes_nothing() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    world.dave_takes_as_authorized(&blob);
    let after_first = dave.list_contacts().unwrap().len();

    let again = world.dave_takes_as_authorized(&blob);

    assert_eq!(again, RekeyOfferTaken::Dropped);
    assert_eq!(dave.list_contacts().unwrap().len(), after_first);
    assert_eq!(dave.rekey_answer_deliveries().unwrap().len(), 1);
}

// @internal
#[test]
fn an_authorized_offer_for_a_contact_met_again_in_person_folds_the_old_contact_into_the_new_one() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let (_, phone_at_dave) = crate::common::meeting::meet(&world.phone, dave, true);
    let met = dave.get_contact(&phone_at_dave).unwrap().unwrap();

    let taken = world.dave_takes_as_authorized(&blob);

    assert_eq!(
        taken,
        RekeyOfferTaken::Accepted {
            contact_id: phone_at_dave.clone()
        }
    );
    assert!(dave.get_contact(&old_id).unwrap().is_none());
    let folded = dave.get_contact(&phone_at_dave).unwrap().unwrap();
    assert_eq!(
        folded.shared_key().unwrap().as_bytes(),
        met.shared_key().unwrap().as_bytes(),
        "the meeting's relationship stays"
    );
    assert!(
        dave.rekey_answer_deliveries().unwrap().is_empty(),
        "no answer: the meeting already joined them"
    );
}

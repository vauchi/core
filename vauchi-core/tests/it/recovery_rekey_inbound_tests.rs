// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact takes in a rekey offer and accepts it by itself when it is
//! provably the recovered contact's (vauchi/private#796, D18): the new
//! identity signed it, its proof opens for one of this device's contacts
//! and names that same identity, and either enough of the vouchers carry a
//! guardian token the old key signed, or enough of them are this device's
//! own contacts. Anything else is dropped without a trace; nobody is asked.
//! The proof is judged without its 192-hour expiry (D21).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::recovery::rekey::RekeyOffer;
use vauchi_core::{Identity, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};
use crate::common::two_recipient::{Recipient, add_recipient};

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

/// Someone claims Alice's old key with vouches from three of her friends,
/// who are no guardians, and offers Dave a rekey. Returns Dave and the
/// offer; `friends_of_dave` makes the friends Dave's contacts too.
fn offered_by_a_claim_of_friends(friends_of_dave: bool) -> (Recipient, Vec<u8>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let dave = add_recipient(&alice, &alice_pk, "Dave");
    let dave_pk = signing_pk(dave.vauchi());
    let friends: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("F{index}")))
        .collect();
    if friends_of_dave {
        for friend in &friends {
            dave.vauchi()
                .add_contact(Contact::from_exchange(
                    signing_pk(friend.vauchi()),
                    ContactCard::new("Friend"),
                    SymmetricKey::generate(),
                    0,
                ))
                .unwrap();
        }
    }
    let claimant = create_vauchi_with_identity("Claimant");
    let claim = claimant
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();
    for friend in &friends {
        let voucher = friend
            .vauchi()
            .create_voucher_from_claim_b64(&claim)
            .unwrap();
        claimant.add_recovery_voucher_b64(&voucher).unwrap();
    }
    let dave_at_claimant = Contact::from_exchange(
        dave_pk,
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    let id = dave_at_claimant.id().to_string();
    claimant.add_contact(dave_at_claimant).unwrap();
    claimant
        .storage()
        .recovery()
        .mark_rekey_pending(&id)
        .unwrap();
    let blob = claimant.rekey_offer_deliveries().unwrap()[0].blob.clone();
    (dave, blob)
}

// @internal
#[test]
fn an_offer_alices_guardians_vouched_for_is_accepted_without_asking() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();

    let taken = dave.take_rekey_offer(&blob).unwrap();

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
fn an_offer_backed_only_by_vouchers_dave_cannot_place_is_dropped() {
    let (dave, blob) = offered_by_a_claim_of_friends(false);
    let alice_id = dave.sharer_id().to_string();

    let taken = dave.vauchi().take_rekey_offer(&blob).unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
    assert!(dave.vauchi().get_contact(&alice_id).unwrap().is_some());
}

// @internal
#[test]
fn an_offer_daves_own_contacts_vouched_for_is_accepted() {
    let (dave, blob) = offered_by_a_claim_of_friends(true);

    let taken = dave.vauchi().take_rekey_offer(&blob).unwrap();

    assert!(
        matches!(taken, RekeyOfferTaken::Accepted { .. }),
        "{taken:?}"
    );
}

// @internal
#[test]
fn an_offer_whose_proof_is_past_its_window_is_still_accepted() {
    let (world, blob) = offered();
    world.dave_clock.advance(Duration::from_secs(30 * 86_400));

    let taken = world.dave.vauchi().take_rekey_offer(&blob).unwrap();

    assert!(
        matches!(taken, RekeyOfferTaken::Accepted { .. }),
        "{taken:?}"
    );
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
            .get_contact(world.dave.sharer_id())
            .unwrap()
            .is_some()
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
    dave.take_rekey_offer(&blob).unwrap();
    let after_first = dave.list_contacts().unwrap().len();

    let again = dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(again, RekeyOfferTaken::Dropped);
    assert_eq!(dave.list_contacts().unwrap().len(), after_first);
    assert_eq!(dave.rekey_answer_deliveries().unwrap().len(), 1);
}

// @internal
#[test]
fn an_offer_for_a_contact_met_again_in_person_folds_the_old_contact_into_the_new_one() {
    let (world, blob) = offered();
    let dave = world.dave.vauchi();
    let old_id = world.dave.sharer_id().to_string();
    let (_, phone_at_dave) = crate::common::meeting::meet(&world.phone, dave, true);
    let met = dave.get_contact(&phone_at_dave).unwrap().unwrap();

    let taken = dave.take_rekey_offer(&blob).unwrap();

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

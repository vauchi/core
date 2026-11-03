// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact that accepts a rekey (vauchi/private#796 step 5b;
//! `recovery_acceptance.spthy`, John_answers) answers with its own
//! exchange key, signed and naming the offer's, to the mailbox the new
//! phone polls; it moves the recovered contact to the new key with the
//! shared key both sides derive, and a fresh ratchet. The answer goes
//! again each day, for at most 14 days (D18).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::clock::FakeClock;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::recovery::rekey::{RekeyAnswer, RekeyOffer, rekey_shared_key};
use vauchi_core::{Contact, ContactCard, Identity, Storage, SymmetricKey, Vauchi};

use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};

const DAY: Duration = Duration::from_secs(86_400);

struct Accepted {
    world: World,
    offer: RekeyOffer,
    old_id: String,
    old_shared: SymmetricKey,
}

/// Alice's restored phone has offered Dave a rekey, and Dave has taken it
/// in, which accepted it.
fn accepted() -> Accepted {
    let mut phone =
        Vauchi::in_memory_with_clock(Arc::new(FakeClock::new(SystemTime::now()))).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let blob = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();
    let old_id = world.dave.sharer_id().to_string();
    let old_shared = world
        .dave
        .vauchi()
        .get_contact(&old_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .unwrap()
        .clone();
    world.dave_takes_as_authorized(&blob);
    Accepted {
        offer: RekeyOffer::decode(&blob).unwrap(),
        world,
        old_id,
        old_shared,
    }
}

// @internal
#[test]
fn accepting_moves_alice_to_her_new_key_with_the_key_both_sides_derive() {
    let a = accepted();
    let dave = a.world.dave.vauchi();
    let phone_pk = signing_pk(&a.world.phone);
    let new_id = hex::encode(phone_pk);

    assert!(dave.get_contact(&a.old_id).unwrap().is_none());
    let alice = dave.get_contact(&new_id).unwrap().unwrap();
    assert_eq!(alice.public_key(), Some(&phone_pk));
    let answer = RekeyAnswer::decode(&dave.rekey_answer_deliveries().unwrap()[0].blob).unwrap();
    let (secret, _) = a
        .world
        .phone
        .storage()
        .recovery()
        .rekey_exchange(&a.world.dave_id)
        .unwrap()
        .unwrap();
    let at_phone = rekey_shared_key(
        &X3DHKeyPair::from_bytes(*secret),
        answer.exchange_key(),
        &phone_pk,
        &signing_pk(dave),
    )
    .unwrap();
    assert_eq!(alice.shared_key().unwrap().as_bytes(), at_phone.as_bytes());
    assert!(
        dave.storage()
            .ratchets()
            .load_ratchet_state(&new_id)
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn the_answer_names_the_offer_and_goes_to_the_mailbox_the_new_phone_polls() {
    let a = accepted();
    let dave = a.world.dave.vauchi();
    let phone_pk = signing_pk(&a.world.phone);

    let deliveries = dave.rekey_answer_deliveries().unwrap();

    assert_eq!(deliveries.len(), 1);
    let today = current_day_epoch(dave.clock().unix_seconds());
    assert_eq!(
        deliveries[0].token_hex,
        token_hex(&compute_mailbox_token(
            a.old_shared.as_bytes(),
            &phone_pk,
            today
        ))
    );
    let answer = RekeyAnswer::decode(&deliveries[0].blob).unwrap();
    assert!(answer.verify(&signing_pk(dave), a.offer.exchange_key()));
}

// @internal
#[test]
fn an_unanswered_answer_goes_again_after_a_day() {
    let a = accepted();
    let dave = a.world.dave.vauchi();
    let first = dave.rekey_answer_deliveries().unwrap();
    dave.record_rekey_answer_sent(&first[0].contact_id).unwrap();

    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
    a.world.dave_clock.advance(DAY);
    let again = dave.rekey_answer_deliveries().unwrap();

    assert_eq!(again.len(), 1);
    assert_eq!(again[0].blob, first[0].blob);
}

// @internal
#[test]
fn an_answer_goes_for_fourteen_days_and_no_longer() {
    let a = accepted();
    let dave = a.world.dave.vauchi();

    a.world
        .dave_clock
        .advance(DAY * 14 - Duration::from_secs(1));
    assert_eq!(dave.rekey_answer_deliveries().unwrap().len(), 1);

    a.world.dave_clock.advance(Duration::from_secs(1));
    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
}

// @internal
#[test]
fn a_kept_answer_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = Contact::from_exchange(
        *Identity::create("Alice", 0).signing_public_key(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    let old_shared = SymmetricKey::generate();
    storage
        .recovery()
        .keep_rekey_answer(contact.id(), b"answer bytes", &old_shared)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    let kept = storage.recovery().kept_rekey_answers().unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0, contact.id());
    assert_eq!(kept[0].1, b"answer bytes");
    assert_eq!(kept[0].2.as_bytes(), old_shared.as_bytes());
}

// @internal
#[test]
fn an_answer_kept_before_its_time_was_recorded_gets_fourteen_days_from_now() {
    let a = accepted();
    let dave = a.world.dave.vauchi();
    dave.storage()
        .connection()
        .execute("UPDATE recovery_rekey_answers SET kept_at = 0", [])
        .unwrap();
    a.world.dave_clock.advance(DAY * 30);

    assert_eq!(dave.rekey_answer_deliveries().unwrap().len(), 1);
    a.world.dave_clock.advance(DAY * 14);
    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
}

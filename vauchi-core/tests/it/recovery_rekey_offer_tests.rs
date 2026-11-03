// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The new phone offers each restored contact a rekey (vauchi/private#796
//! step 4b): one signed offer per contact waiting for its rekey, carrying
//! the recovery proof, addressed to the mailbox that contact already polls
//! for its old relationship, and sent again each day until answered with
//! the same exchange key.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::recovery::rekey::RekeyOffer;
use vauchi_core::{Storage, SymmetricKey, Vauchi};

use crate::common::meeting::signing_pk;
use crate::common::recovered_phone::{World, recovered};

const DAY: Duration = Duration::from_secs(86_400);

/// Alice's new phone on `clock`, with the backup restored.
fn restored(clock: &Arc<FakeClock>) -> World {
    let mut phone = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let world = recovered(phone, None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    world
}

/// The offers due now to `contact_id` alone.
fn offers_to(world: &World, contact_id: &str) -> Vec<vauchi_core::api::RekeyDelivery> {
    world
        .phone
        .rekey_offer_deliveries()
        .unwrap()
        .into_iter()
        .filter(|delivery| delivery.contact_id == contact_id)
        .collect()
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(SystemTime::now()))
}

// @internal
#[test]
fn each_waiting_contact_gets_one_offer_in_the_mailbox_it_polls() {
    let clock = clock();
    let world = restored(&clock);

    let deliveries = world.phone.rekey_offer_deliveries().unwrap();

    let mut offered: Vec<&str> = deliveries
        .iter()
        .map(|delivery| delivery.contact_id.as_str())
        .collect();
    offered.sort_unstable();
    let mut waiting: Vec<&str> = world
        .guardians_at_phone
        .iter()
        .map(String::as_str)
        .chain([world.dave_id.as_str()])
        .collect();
    waiting.sort_unstable();
    assert_eq!(offered, waiting, "Dave and the guardians met again");
    let deliveries = offers_to(&world, &world.dave_id);
    let dave_at_phone = world.phone.get_contact(&world.dave_id).unwrap().unwrap();
    let old_shared = dave_at_phone.shared_key().unwrap();
    let dave_pk = signing_pk(world.dave.vauchi());
    let today = current_day_epoch(world.phone.clock().unix_seconds());
    assert_eq!(
        deliveries[0].token_hex,
        token_hex(&compute_mailbox_token(
            old_shared.as_bytes(),
            &dave_pk,
            today
        )),
        "the mailbox Dave polls for Alice"
    );
}

// @internal
#[test]
fn the_offer_is_signed_by_the_new_identity_and_carries_a_proof_dave_can_check() {
    let clock = clock();
    let world = restored(&clock);

    let deliveries = offers_to(&world, &world.dave_id);
    let offer = RekeyOffer::decode(&deliveries[0].blob).unwrap();

    assert!(offer.verify());
    assert_eq!(offer.new_pk(), &signing_pk(&world.phone));
    let (proof, _verdict) = world
        .dave
        .vauchi()
        .verify_recovery_proof(offer.proof())
        .unwrap();
    assert_eq!(proof.new_pk(), &signing_pk(&world.phone));
    assert_eq!(hex::encode(proof.old_pk()), world.dave.sharer_id());
}

// @internal
#[test]
fn an_offer_goes_out_once_a_day_until_answered_with_the_same_exchange_key() {
    let clock = clock();
    let world = restored(&clock);
    let first = offers_to(&world, &world.dave_id);
    world
        .phone
        .record_rekey_offer_sent(&first[0].contact_id)
        .unwrap();

    assert!(offers_to(&world, &world.dave_id).is_empty());
    clock.advance(DAY - Duration::from_secs(1));
    assert!(offers_to(&world, &world.dave_id).is_empty());

    clock.advance(Duration::from_secs(1));
    let again = offers_to(&world, &world.dave_id);

    assert_eq!(again.len(), 1);
    assert_eq!(
        RekeyOffer::decode(&again[0].blob).unwrap().exchange_key(),
        RekeyOffer::decode(&first[0].blob).unwrap().exchange_key()
    );
}

// @internal
#[test]
fn without_a_recovery_proof_no_offer_is_made() {
    let vauchi = crate::common::helpers::create_vauchi_with_identity("Alice");
    let contact = Contact::from_exchange(
        *vauchi_core::Identity::create("Dave", 0).signing_public_key(),
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    vauchi.storage().recovery().mark_rekey_pending(&id).unwrap();

    assert!(vauchi.rekey_offer_deliveries().unwrap().is_empty());
}

// @internal
#[test]
fn the_kept_exchange_key_and_proof_survive_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = Contact::from_exchange(
        *vauchi_core::Identity::create("Dave", 0).signing_public_key(),
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    let exchange = X3DHKeyPair::generate();
    storage
        .recovery()
        .save_rekey_exchange(contact.id(), &exchange.secret_bytes())
        .unwrap();
    storage.recovery().save_own_proof(b"sealed proof").unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    let kept = storage
        .recovery()
        .rekey_exchange(contact.id())
        .unwrap()
        .unwrap();
    assert_eq!(
        X3DHKeyPair::from_bytes(*kept.0).public_key(),
        exchange.public_key()
    );
    assert_eq!(
        storage.recovery().load_own_proof().unwrap().as_deref(),
        Some(b"sealed proof".as_slice())
    );
}

// @internal
#[test]
fn offers_go_for_fourteen_days_after_the_restore_and_no_longer() {
    let clock = clock();
    let world = restored(&clock);

    clock.advance(DAY * 14 - Duration::from_secs(1));
    assert_eq!(offers_to(&world, &world.dave_id).len(), 1);

    clock.advance(Duration::from_secs(1));
    assert!(world.phone.rekey_offer_deliveries().unwrap().is_empty());
}

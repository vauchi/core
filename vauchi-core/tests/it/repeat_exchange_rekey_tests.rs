// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Repeat (2nd-to-nth) in-person exchange — upsert, no rekey.
//!
//! A 2nd in-person exchange between the SAME two parties re-exchanges fresh
//! cards. It must UPSERT the peer's (updated) card — never drop it — and
//! keep the channel the first exchange keyed: an exchange never rekeys an
//! existing contact (private#941, ship checklist SC-06, replacing the rekey
//! of the 2026-06-27 repeat-exchange decision). The BLE, QR, and multi-stage completion paths all
//! route through `Vauchi::save_exchanged_contact` so the three stay consistent
//! (ADR-021/043: the reuse-vs-rekey logic lives in core, not per-transport in
//! the humble frontends).
//!
//! Problem record: 2026-06-27-repeat-exchange-sync-propagation.
//! Feature: contact_exchange.feature, sync_updates.feature

use crate::common;

use common::helpers::{create_vauchi_with_card, setup_ratchets};
use vauchi_core::{
    Contact, ContactField, FieldType, SymmetricKey, api::process_single_card_update,
};

/// A 2nd exchange of the same pair updates the peer's card and keeps the
/// channel, so a later card-update still round-trips on it.
///
/// RED before `save_exchanged_contact` upserts: the historical BLE/multistage
/// path called `add_contact`, which rejects a duplicate id and returned BEFORE
/// persisting the ratchet — silently dropping the peer's updated card and
/// leaving the old channel.
// @scenario: contact_exchange :: Repeat in-person exchange of the same pair
// @scenario: sync_updates :: Update propagates after a re-exchange
#[test]
fn second_exchange_upserts_card_and_sync_round_trips_on_the_kept_channel() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@v1.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card_v1 = bob.own_card().unwrap().unwrap();

    // ── Exchange #1 — establish the channel via the unified completion routine.
    let secret1 = SymmetricKey::generate();
    let (a_rat1, b_rat1) = setup_ratchets(&secret1);

    let bob_at_alice_v1 = Contact::from_exchange(bob_pk, bob_card_v1.clone(), secret1.clone(), 0);
    let bob_id = bob_at_alice_v1.id().to_string();
    alice
        .save_exchanged_contact(&bob_at_alice_v1, &a_rat1, true)
        .expect("first exchange must persist the new contact");

    let alice_at_bob_v1 = Contact::from_exchange(alice_pk, alice_card.clone(), secret1.clone(), 0);
    let alice_id = alice_at_bob_v1.id().to_string();
    bob.save_exchanged_contact(&alice_at_bob_v1, &b_rat1, false)
        .expect("first exchange must persist the new contact");

    // Bob's card changes between exchanges (the update a repeat carries).
    let bob_email_id = bob_card_v1
        .fields()
        .iter()
        .find(|f| f.label() == "personal")
        .unwrap()
        .id()
        .to_string();
    let mut bob_card_v2 = bob_card_v1.clone();
    bob_card_v2
        .update_field_value(&bob_email_id, "bob@v2.com", 1)
        .unwrap();

    // ── Exchange #2 — SAME pair, fresh secret + ratchets, Bob's updated card.
    let secret2 = SymmetricKey::generate();
    let (a_rat2, b_rat2) = setup_ratchets(&secret2);

    let bob_at_alice_v2 = Contact::from_exchange(bob_pk, bob_card_v2.clone(), secret2.clone(), 2);
    alice
        .save_exchanged_contact(&bob_at_alice_v2, &a_rat2, true)
        .expect("a repeat exchange must upsert, not reject");
    let alice_at_bob_v2 = Contact::from_exchange(alice_pk, alice_card.clone(), secret2.clone(), 2);
    bob.save_exchanged_contact(&alice_at_bob_v2, &b_rat2, false)
        .expect("a repeat exchange must upsert, not reject");

    // ASSERT 1 (the bug): the repeat UPDATED Bob's card; it was not dropped.
    let stored_bob = alice.get_contact(&bob_id).unwrap().unwrap();
    let stored_email = stored_bob
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "personal")
        .expect("personal email field must exist");
    assert_eq!(
        stored_email.value(),
        "bob@v2.com",
        "a 2nd exchange of the same pair must UPDATE the peer's card, not drop it"
    );

    // ASSERT 2 (the goal): a later card-update round-trips to the peer on
    // the channel the first exchange keyed.
    let alice_old = alice.own_card().unwrap().unwrap();
    let alice_email_id = alice_old
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap()
        .id()
        .to_string();
    let mut alice_new = alice_old.clone();
    alice_new
        .update_field_value(&alice_email_id, "alice@new.com", 3)
        .unwrap();
    alice.update_own_card(&alice_new).unwrap();
    let alice_new = alice.own_card().unwrap().unwrap();

    let encrypted =
        common::card_update::seal_update_default(&alice, &bob_id, &alice_old, &alice_new);
    process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id,
        &encrypted,
    )
    .expect("peer must decrypt the update on the kept channel");

    let alice_at_bob_final = bob.get_contact(&alice_id).unwrap().unwrap();
    let final_email = alice_at_bob_final
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .expect("work email must exist at the peer");
    assert_eq!(
        final_email.value(),
        "alice@new.com",
        "the later update must reflect at the peer"
    );
}

/// A 1st exchange via `save_exchanged_contact` inserts the contact and a
/// card-update round-trips — guards that the unified routine did not regress
/// the new-contact (insert) path.
// @scenario: contact_exchange :: First exchange persists the contact
#[test]
fn first_exchange_inserts_contact_and_card_update_round_trips() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@email.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    let secret = SymmetricKey::generate();
    let (a_rat, b_rat) = setup_ratchets(&secret);

    let bob_contact = Contact::from_exchange(bob_pk, bob_card, secret.clone(), 0);
    let bob_id = bob_contact.id().to_string();
    alice
        .save_exchanged_contact(&bob_contact, &a_rat, true)
        .unwrap();

    let alice_contact = Contact::from_exchange(alice_pk, alice_card.clone(), secret.clone(), 0);
    let alice_id = alice_contact.id().to_string();
    bob.save_exchanged_contact(&alice_contact, &b_rat, false)
        .unwrap();

    // Contact is present after the first exchange.
    assert!(
        alice.get_contact(&bob_id).unwrap().is_some(),
        "first exchange must store the contact"
    );

    // And a card-update round-trips.
    let old_card = alice.own_card().unwrap().unwrap();
    alice
        .add_own_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://alice.example",
            0,
        ))
        .unwrap();
    let new_card = alice.own_card().unwrap().unwrap();
    let encrypted = common::card_update::seal_update_default(&alice, &bob_id, &old_card, &new_card);
    process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id,
        &encrypted,
    )
    .expect("first-exchange card-update must round-trip");
    let alice_at_bob = bob.get_contact(&alice_id).unwrap().unwrap();
    assert!(
        alice_at_bob
            .card()
            .fields()
            .iter()
            .any(|f| f.label() == "blog" && f.value() == "https://alice.example"),
        "first-exchange card-update must deliver the new blog field to the peer"
    );
}

/// A card-update in flight across a repeat exchange still applies: the
/// repeat keeps the channel, so the update sealed before it decrypts after
/// it, and a later update delivers too (private#941, SC-06).
// @internal
#[test]
fn an_update_in_flight_across_a_repeat_exchange_still_applies() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@email.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // ── Exchange #1 (ratchet1).
    let secret1 = SymmetricKey::generate();
    let (a_rat1, b_rat1) = setup_ratchets(&secret1);
    let bob_at_alice = Contact::from_exchange(bob_pk, bob_card.clone(), secret1.clone(), 0);
    let bob_id = bob_at_alice.id().to_string();
    alice
        .save_exchanged_contact(&bob_at_alice, &a_rat1, true)
        .unwrap();
    let alice_at_bob = Contact::from_exchange(alice_pk, alice_card.clone(), secret1.clone(), 0);
    let alice_id = alice_at_bob.id().to_string();
    bob.save_exchanged_contact(&alice_at_bob, &b_rat1, false)
        .unwrap();

    // Alice sends an in-flight update U1 under ratchet1 (Bob has not received it).
    let old1 = alice.own_card().unwrap().unwrap();
    let work_id = old1
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap()
        .id()
        .to_string();
    let mut new1 = old1.clone();
    new1.update_field_value(&work_id, "alice@inflight.com", 1)
        .unwrap();
    alice.update_own_card(&new1).unwrap();
    let new1 = alice.own_card().unwrap().unwrap();
    let u1 = common::card_update::seal_update_default(&alice, &bob_id, &old1, &new1);

    // ── Exchange #2 — both keep ratchet1; ratchet2 is offered and not taken.
    let secret2 = SymmetricKey::generate();
    let (a_rat2, b_rat2) = setup_ratchets(&secret2);
    alice
        .save_exchanged_contact(
            &Contact::from_exchange(bob_pk, bob_card.clone(), secret2.clone(), 2),
            &a_rat2,
            true,
        )
        .unwrap();
    bob.save_exchanged_contact(
        &Contact::from_exchange(alice_pk, alice_card.clone(), secret2.clone(), 2),
        &b_rat2,
        false,
    )
    .unwrap();

    // Bob receives U1 after the repeat exchange; the kept channel opens it.
    process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &u1)
        .expect("an update sealed before a repeat exchange still decrypts after it");
    let alice_at_bob_after = bob.get_contact(&alice_id).unwrap().unwrap();
    let work_after = alice_at_bob_after
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .expect("work field must exist");
    assert_eq!(work_after.value(), "alice@inflight.com");
    // A later update on the same channel delivers too.
    let old2 = alice.own_card().unwrap().unwrap();
    alice
        .add_own_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://alice.example",
            3,
        ))
        .unwrap();
    let new2 = alice.own_card().unwrap().unwrap();
    let u2 = common::card_update::seal_update_default(&alice, &bob_id, &old2, &new2);
    process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &u2)
        .expect("the kept channel delivers the next update");
    let final_at_bob = bob.get_contact(&alice_id).unwrap().unwrap();
    assert!(
        final_at_bob
            .card()
            .fields()
            .iter()
            .any(|f| f.label() == "blog"),
        "the later update must apply at the peer"
    );
}

/// A first exchange on a multi-device identity journals
/// `SyncItem::ContactAdded` for the user's other linked devices — the
/// same recording `Vauchi::add_contact` does. RED while the exchange
/// persist seam bare-saves without `record_sync_item`: the exchanged
/// contact never reaches the user's tablet
/// (2026-06-27-repeat-exchange-sync-propagation, journal gap).
// @scenario: sync_updates :: Exchanged contact reaches linked devices
#[test]
fn first_exchange_journals_contact_added_for_linked_devices() {
    use vauchi_core::api::sync::DeviceSyncOrchestrator;
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
    use vauchi_core::sync::SyncItem;

    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "a@w.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "b@v1.com")]);

    // Two linked devices — record_sync_item is a deliberate no-op below 2.
    const SEED: [u8; 32] = [7u8; 32];
    let signing = SigningKeyPair::from_seed(&SEED);
    let mut registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &signing,
    );
    registry
        .add_device_unsigned(DeviceInfo::derive(&SEED, 1, "tablet".into(), 0).to_registered(&SEED))
        .unwrap();
    alice
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();

    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let bob_card = bob.own_card().unwrap().unwrap();
    let secret = SymmetricKey::generate();
    let (a_rat, _) = setup_ratchets(&secret);
    let bob_at_alice = Contact::from_exchange(bob_pk, bob_card, secret, 0);
    let bob_id = bob_at_alice.id().to_string();
    alice
        .save_exchanged_contact(&bob_at_alice, &a_rat, true)
        .unwrap();

    let device_info = alice.identity().unwrap().create_device_info(0);
    let orchestrator =
        DeviceSyncOrchestrator::load(alice.storage(), device_info, registry).unwrap();
    let queued_added_ids: Vec<String> = orchestrator
        .devices_with_pending()
        .iter()
        .flat_map(|d| orchestrator.pending_for_device(d).to_vec())
        .filter_map(|item| match item {
            SyncItem::ContactAdded { contact_data, .. } => Some(contact_data.id.clone()),
            _ => None,
        })
        .collect();
    assert!(
        queued_added_ids.contains(&bob_id),
        "an exchange on a multi-device identity must journal ContactAdded \
         for the linked devices, journaled ids: {queued_added_ids:?}"
    );
}

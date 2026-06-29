// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Repeat (2nd-to-nth) in-person exchange — rekey + upsert.
//!
//! A 2nd in-person exchange between the SAME two parties re-exchanges fresh
//! cards and a fresh shared secret. Per the 2026-06-27 repeat-exchange
//! decision it must UPSERT the peer's (updated) card and REKEY the channel —
//! never drop the new card. The BLE, QR, and multi-stage completion paths all
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

/// A 2nd exchange of the same pair updates the peer's card and rekeys, so a
/// later card-update still round-trips under the new channel.
///
/// RED before `save_exchanged_contact` upserts: the historical BLE/multistage
/// path called `add_contact`, which rejects a duplicate id and returned BEFORE
/// persisting the ratchet — silently dropping the peer's updated card and
/// leaving the old channel.
// @scenario: contact_exchange :: Repeat in-person exchange of the same pair
// @scenario: sync_updates :: Update propagates after a re-exchange
#[test]
fn second_exchange_upserts_card_and_rekeys_so_sync_round_trips() {
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

    // ASSERT 2 (the goal): a card-update prepared under the post-rekey ratchet
    // round-trips to the peer — the re-exchange propagates as a sync update.
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
    .expect("peer must decrypt the update under the rekeyed channel");

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
        "the post-rekey update must reflect at the peer"
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

/// A card-update sent under the OLD ratchet, received AFTER a rekey, is
/// gracefully skipped — it does not apply, does not corrupt the rekeyed
/// channel, and a subsequent update under the new ratchet still delivers.
///
/// This pins the in-flight-undecryptable safety net for rekey+upsert: the
/// stale message fails to decrypt (skipped + ACK-dropped by the receive
/// phase), its content is superseded by the in-person re-exchange, and the
/// atomic ratchet-save (only on success) means the failed decrypt never
/// advances the stored ratchet ("ratchet advanced but message lost",
/// Tracker #159). 2026-06-27-repeat-exchange-sync-propagation.
// @scenario: sync_updates :: Stale pre-rekey update is skipped, channel survives
#[test]
fn stale_pre_rekey_update_is_skipped_and_does_not_break_the_rekeyed_channel() {
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

    // ── Exchange #2 — both rekey to ratchet2 (the re-exchange supersedes U1).
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

    // Bob receives the stale U1 (old ratchet) AFTER the rekey → skipped, not applied.
    assert!(
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &u1).is_err(),
        "a stale old-ratchet update after a rekey must fail to decrypt (skipped), not apply"
    );
    let alice_at_bob_after = bob.get_contact(&alice_id).unwrap().unwrap();
    let work_after = alice_at_bob_after
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .expect("work field must exist");
    assert_eq!(
        work_after.value(),
        "alice@old.com",
        "the skipped stale update must NOT mutate the peer's card"
    );

    // The rekeyed channel still delivers: a NEW-ratchet update applies, proving
    // the failed decrypt did not corrupt or advance the stored ratchet.
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
        .expect("the rekeyed channel must still deliver after a stale message was skipped");
    let final_at_bob = bob.get_contact(&alice_id).unwrap().unwrap();
    assert!(
        final_at_bob
            .card()
            .fields()
            .iter()
            .any(|f| f.label() == "blog"),
        "the post-rekey update must apply at the peer"
    );
}

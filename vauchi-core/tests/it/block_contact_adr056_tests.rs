// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Blocking as ADR-056 decides it, points 4 and 5 (private#889): one
//! optional, user-confirmed wipe of the blocked device's copy of my card,
//! group membership stripped and not restored on unblock, and the blocked
//! contact leaving the emergency trusted list (maintainer decision
//! 2026-10-28).

use vauchi_core::sync::SyncItem;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi};

use crate::common::two_recipient::{Recipient, add_recipient, deliver, stored_card_has};

fn alice() -> Vauchi {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice").unwrap();
    alice
}

fn exchanged_contact(name: &str) -> Contact {
    let identity = Identity::create(name, 0);
    Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

/// Alice shares a public "work" email; Dave holds a copy of it.
fn alice_sharing_work_with(name: &str) -> (Vauchi, Recipient) {
    let alice = alice();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    alice
        .set_own_field_public(&own_field_id(&alice, "work"))
        .unwrap();
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let mut dave = add_recipient(&alice, &alice_pk, name);
    alice
        .queue_initial_card_for_contact(&dave.id_at_sharer)
        .unwrap();
    assert_eq!(deliver(&alice, &mut dave), 1, "Dave receives Alice's card");
    assert!(stored_card_has(&dave, "work"), "Dave holds the work email");
    (alice, dave)
}

fn own_field_id(alice: &Vauchi, label: &str) -> String {
    alice
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap()
        .id()
        .to_string()
}

/// Alice with a Mobile field shown only to Family, and Dave in Family.
fn alice_with_dave_in_family() -> (Vauchi, String, String) {
    let alice = alice();
    alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 000 00 00",
            0,
        ))
        .unwrap();
    let mobile_id = own_field_id(&alice, "Mobile");
    let family = alice.create_group("Family").unwrap();
    alice
        .set_group_field_visibility(family.id(), &mobile_id, true)
        .unwrap();
    let dave = exchanged_contact("Dave");
    let dave_id = dave.id().to_string();
    alice.add_contact(dave).unwrap();
    alice.add_contact_to_group(family.id(), &dave_id).unwrap();
    assert!(
        alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Family membership shows Mobile"
    );
    (alice, dave_id, mobile_id)
}

// @scenario: visibility_control :: Block contact removes all their visibility
#[test]
fn blocking_takes_the_contact_out_of_every_group_and_unblocking_leaves_them_out() {
    let (alice, dave_id, mobile_id) = alice_with_dave_in_family();

    alice.block_contact(&dave_id).unwrap();

    assert!(
        alice.get_groups_for_contact(&dave_id).unwrap().is_empty(),
        "a blocked contact belongs to no group"
    );
    assert!(
        !alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Mobile is no longer visible to Dave"
    );

    alice.unblock_contact(&dave_id).unwrap();

    assert!(
        alice.get_groups_for_contact(&dave_id).unwrap().is_empty(),
        "unblocking does not restore membership (ADR-056 point 5)"
    );
    assert!(
        !alice
            .get_effective_field_visibility(&dave_id, &mobile_id)
            .unwrap(),
        "Mobile stays hidden until Alice grants it again"
    );
}

// @scenario: visibility_control :: Blocking offers a final wipe of my info
#[test]
fn blocking_with_the_wipe_sends_one_last_update_that_empties_the_card_and_nothing_after() {
    let (alice, mut dave) = alice_sharing_work_with("Dave");

    let report = alice
        .block_contact_and_report(&dave.id_at_sharer, true)
        .unwrap();

    assert!(report.wipe_queued, "the wipe went out");
    assert_eq!(deliver(&alice, &mut dave), 1, "exactly one final update");
    assert!(
        !stored_card_has(&dave, "work"),
        "Dave's copy of Alice's card is empty"
    );

    let before = alice.own_card().unwrap().unwrap();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "home", "a@home.ch", 0))
        .unwrap();
    let after = alice.own_card().unwrap().unwrap();
    assert_eq!(
        alice.propagate_card_update(&before, &after).unwrap(),
        0,
        "no later edit reaches a blocked contact"
    );
    assert_eq!(
        deliver(&alice, &mut dave),
        0,
        "nothing is ever sent afterwards"
    );
}

// @scenario: visibility_control :: Block contact removes all their visibility
#[test]
fn blocking_without_the_wipe_sends_nothing_and_leaves_their_copy_as_it_was() {
    let (alice, mut dave) = alice_sharing_work_with("Dave");

    let report = alice
        .block_contact_and_report(&dave.id_at_sharer, false)
        .unwrap();

    assert!(!report.wipe_queued, "no wipe was asked for");
    assert_eq!(
        deliver(&alice, &mut dave),
        0,
        "a silent block sends nothing"
    );
    assert!(
        stored_card_has(&dave, "work"),
        "data already delivered cannot be unsent (ADR-056 point 4)"
    );
    assert!(
        alice
            .get_contact(&dave.id_at_sharer)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
}

// @scenario: visibility_control :: Blocking offers a final wipe of my info
#[test]
fn blocking_a_contact_without_a_session_still_blocks_and_reports_the_wipe_as_not_sent() {
    let alice = alice();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let dave = exchanged_contact("Dave");
    let dave_id = dave.id().to_string();
    alice.add_contact(dave).unwrap();

    let report = alice.block_contact_and_report(&dave_id, true).unwrap();

    assert!(
        !report.wipe_queued,
        "no ratchet, so no wipe could be sealed"
    );
    assert!(alice.get_contact(&dave_id).unwrap().unwrap().is_blocked());
    assert_eq!(
        alice
            .storage()
            .pending()
            .count_pending_updates(&dave_id)
            .unwrap(),
        0,
        "nothing half-prepared is left in the queue"
    );
}

// @scenario: emergency_broadcast :: Blocked contact in trusted list
#[test]
fn blocking_removes_the_contact_from_the_emergency_trusted_list_and_says_so() {
    let mut alice = alice();
    let dave = exchanged_contact("Dave");
    let dave_id = dave.id().to_string();
    alice.add_contact(dave).unwrap();
    let bob = exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    alice.add_contact(bob).unwrap();
    alice
        .configure_emergency_broadcast(vec![dave_id.clone(), bob_id.clone()], "Help".into(), false)
        .unwrap();

    let report = alice.block_contact_and_report(&dave_id, false).unwrap();

    assert!(report.removed_from_emergency_contacts, "Alice is told");
    assert_eq!(
        alice
            .load_emergency_config()
            .unwrap()
            .unwrap()
            .trusted_contact_ids,
        vec![bob_id.clone()],
        "Dave is no longer trusted; Bob still is"
    );

    let report = alice.block_contact_and_report(&bob_id, false).unwrap();

    assert!(report.removed_from_emergency_contacts);
    assert!(
        alice.load_emergency_config().unwrap().is_none(),
        "the last trusted contact leaving turns the broadcast off"
    );
}

// @scenario: emergency_broadcast :: Blocked contact in trusted list
#[test]
fn blocking_a_contact_who_was_never_trusted_says_nothing_about_the_emergency_list() {
    let mut alice = alice();
    let dave = exchanged_contact("Dave");
    let dave_id = dave.id().to_string();
    alice.add_contact(dave).unwrap();
    let bob = exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    alice.add_contact(bob).unwrap();
    alice
        .configure_emergency_broadcast(vec![bob_id.clone()], "Help".into(), false)
        .unwrap();

    let report = alice.block_contact_and_report(&dave_id, false).unwrap();

    assert!(!report.removed_from_emergency_contacts);
    assert_eq!(
        alice
            .load_emergency_config()
            .unwrap()
            .unwrap()
            .trusted_contact_ids,
        vec![bob_id]
    );
}

// @scenario: emergency_broadcast :: Blocked contact in trusted list
#[test]
fn a_block_synced_from_another_device_also_drops_the_contact_from_this_devices_trusted_list() {
    let mut alice = alice();
    let dave = exchanged_contact("Dave");
    let dave_id = dave.id().to_string();
    alice.add_contact(dave).unwrap();
    alice
        .configure_emergency_broadcast(vec![dave_id.clone()], "Help".into(), false)
        .unwrap();

    let applied = alice
        .apply_sync_items(vec![SyncItem::ContactBlocked {
            contact_id: dave_id.clone(),
            timestamp: 1_700_000_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(alice.get_contact(&dave_id).unwrap().unwrap().is_blocked());
    assert!(
        alice.load_emergency_config().unwrap().is_none(),
        "this device would otherwise keep alerting a contact the owner blocked elsewhere"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a contact actually receives: only the fields they may see, all of
//! the public ones from the first exchange on, and a batch of visibility
//! changes as one update.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;
use vauchi_core::Vauchi;
use vauchi_core::api::process_single_card_update;
use vauchi_core::contact_card::{ContactField, FieldType};

const WORK_PHONE: &str = "+41 44 000 00 00";
const PERSONAL_PHONE: &str = "+41 79 999 99 99";

/// Delivers every update `from` has queued for `to` and returns how many.
fn deliver(from: &Vauchi, to_id_in_from: &str, to: &Vauchi, from_id_in_to: &str) -> usize {
    let queued = from
        .storage()
        .pending()
        .get_pending_updates(to_id_in_from)
        .unwrap();
    for update in &queued {
        process_single_card_update(
            to.identity().unwrap(),
            to.storage(),
            from_id_in_to,
            &update.payload,
        )
        .unwrap();
    }
    queued.len()
}

fn add_fields(owner: &Vauchi, fields: &[(FieldType, &str, &str)]) -> Vec<String> {
    for (kind, label, value) in fields {
        owner
            .add_own_field(ContactField::new(kind.clone(), label, value, 0))
            .unwrap();
    }
    owner
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .map(|f| f.id().to_string())
        .collect()
}

fn values_held(holder: &Vauchi, contact_id: &str) -> Vec<String> {
    let mut values: Vec<String> = holder
        .get_contact(contact_id)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .map(|f| f.value().to_string())
        .collect();
    values.sort();
    values
}

const FOUR: [(FieldType, &str, &str); 4] = [
    (FieldType::Phone, "Mobile", "+41 79 111 11 11"),
    (FieldType::Phone, "Office", "+41 44 222 22 22"),
    (FieldType::Email, "Personal", "alice@home.example"),
    (FieldType::Email, "Work", "alice@work.example"),
];

// @scenario: visibility_control :: New contacts see fields I made visible to all
#[test]
fn a_new_contact_gets_all_four_public_fields_with_the_first_card() {
    let alice = create_vauchi_with_identity("Alice");
    for id in add_fields(&alice, &FOUR) {
        alice.set_own_field_public(&id).unwrap();
    }
    let eve = create_vauchi_with_identity("Eve");
    let linked = link_contacts(&eve, &alice, "Alice");

    alice
        .queue_initial_card_for_contact(&linked.host_contact_id)
        .unwrap();
    deliver(
        &alice,
        &linked.host_contact_id,
        &eve,
        &linked.peer_contact_id,
    );

    let mut expected: Vec<String> = FOUR.iter().map(|(_, _, v)| v.to_string()).collect();
    expected.sort();
    assert_eq!(values_held(&eve, &linked.peer_contact_id), expected);
}

// @scenario: visibility_control :: Visibility changes are atomic
#[test]
fn three_visibility_changes_reach_bob_as_one_update() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let linked = link_contacts(&bob, &alice, "Alice");
    let ids = add_fields(&alice, &FOUR);
    alice.run_owed_repropagation().unwrap();
    deliver(
        &alice,
        &linked.host_contact_id,
        &bob,
        &linked.peer_contact_id,
    );
    for update in alice
        .storage()
        .pending()
        .get_pending_updates(&linked.host_contact_id)
        .unwrap()
    {
        alice
            .storage()
            .pending()
            .delete_pending_update(&update.id)
            .unwrap();
    }

    for id in &ids[..3] {
        alice.set_own_field_public(id).unwrap();
    }
    alice.run_owed_repropagation().unwrap();
    let updates = deliver(
        &alice,
        &linked.host_contact_id,
        &bob,
        &linked.peer_contact_id,
    );

    assert_eq!(updates, 1);
    let mut expected: Vec<String> = FOUR[..3].iter().map(|(_, _, v)| v.to_string()).collect();
    expected.sort();
    assert_eq!(values_held(&bob, &linked.peer_contact_id), expected);
}

// @scenario: visibility_control :: Contact cannot determine hidden field existence
#[test]
fn dave_holds_the_visible_field_and_nothing_of_the_hidden_one() {
    let alice = create_vauchi_with_identity("Alice");
    let dave = create_vauchi_with_identity("Dave");
    let linked = link_contacts(&dave, &alice, "Alice");
    let (dave_in_alice, alice_in_dave) = (&linked.host_contact_id, &linked.peer_contact_id);
    for (label, value) in [("Work", WORK_PHONE), ("Personal", PERSONAL_PHONE)] {
        alice
            .add_own_field(ContactField::new(FieldType::Phone, label, value, 0))
            .unwrap();
    }
    let work = alice
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == "Work")
        .unwrap()
        .id()
        .to_string();
    alice.set_own_field_public(&work).unwrap();
    alice.run_owed_repropagation().unwrap();

    deliver(&alice, dave_in_alice, &dave, alice_in_dave);

    let alice_at_dave = dave.get_contact(alice_in_dave).unwrap().unwrap();
    let values: Vec<&str> = alice_at_dave
        .card()
        .fields()
        .iter()
        .map(|f| f.value())
        .collect();
    assert_eq!(values, [WORK_PHONE]);
    let everything_dave_holds = format!("{alice_at_dave:?}");
    assert!(!everything_dave_holds.contains(PERSONAL_PHONE));
    assert!(!everything_dave_holds.contains("Personal"));
}

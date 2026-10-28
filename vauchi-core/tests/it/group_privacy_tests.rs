// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A group decides what a contact sees, but its name never reaches them:
//! Bob receives the field his group grants and nothing that names it.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;
use vauchi_core::api::process_single_card_update;
use vauchi_core::contact_card::{ContactField, FieldType};

const GROUP_NAME: &str = "Annoying People";
const PHONE: &str = "+41 79 123 45 67";

// @scenario: visibility_labels :: Labels exist only on my devices
// @scenario: visibility_labels :: Labels are not shared with contacts
#[test]
fn bob_gets_the_field_his_group_grants_but_never_the_group_name() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let linked = link_contacts(&bob, &alice, "Alice");
    let (bob_in_alice, alice_in_bob) = (&linked.host_contact_id, &linked.peer_contact_id);
    alice
        .add_own_field(ContactField::new(FieldType::Phone, "Mobile", PHONE, 0))
        .unwrap();
    let phone_id = alice.own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    let group = alice.create_group(GROUP_NAME).unwrap();
    alice
        .set_group_field_visibility_and_repropagate(group.id(), &phone_id, true)
        .unwrap();
    alice
        .add_contact_to_group_and_repropagate(group.id(), bob_in_alice)
        .unwrap();
    alice.run_owed_repropagation().unwrap();

    for update in alice
        .storage()
        .pending()
        .get_pending_updates(bob_in_alice)
        .unwrap()
    {
        process_single_card_update(
            bob.identity().unwrap(),
            bob.storage(),
            alice_in_bob,
            &update.payload,
        )
        .unwrap();
    }

    let alice_at_bob = format!("{:?}", bob.get_contact(alice_in_bob).unwrap().unwrap());
    assert!(
        alice_at_bob.contains(PHONE),
        "the update arrived: {alice_at_bob}"
    );
    assert!(!alice_at_bob.contains(GROUP_NAME), "{alice_at_bob}");
    assert!(bob.list_groups().unwrap().is_empty());
}

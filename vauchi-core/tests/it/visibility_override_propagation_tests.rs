// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A per-contact visibility override reaches that contact on the next
//! sync tick, granted or revoked (vauchi/private#691 found it: a revoke
//! made while the contact was offline was never sent).

use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

/// Alice with one ratcheted, exchanged contact.
fn alice_with_friend() -> (Vauchi, String) {
    let wb = create_vauchi_with_identity("Alice");
    let friend = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let friend_id = friend.id().to_string();
    wb.add_contact(friend).unwrap();
    let dh = vauchi_core::exchange::X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(
        &friend_id,
        &vauchi_core::crypto::SymmetricKey::generate(),
        *dh.public_key(),
    )
    .unwrap();
    (wb, friend_id)
}

fn pending(wb: &Vauchi) -> usize {
    wb.storage().pending().count_all_pending_updates().unwrap()
}

// @scenario: visibility_control :: Granting visibility sends update to contact
#[test]
fn a_per_contact_grant_and_revoke_each_queue_an_update_on_the_next_tick() {
    let (wb, friend_id) = alice_with_friend();
    let field = ContactField::new(FieldType::Phone, "Phone", "+41 79 000 00 00", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();
    wb.run_owed_repropagation().unwrap();
    let after_add = pending(&wb);

    wb.set_contact_visibility_override(&friend_id, &field_id, true)
        .unwrap();
    wb.run_owed_repropagation().unwrap();
    assert_eq!(pending(&wb), after_add + 1, "the grant is sent");

    wb.set_contact_visibility_override(&friend_id, &field_id, false)
        .unwrap();
    wb.run_owed_repropagation().unwrap();
    assert_eq!(pending(&wb), after_add + 2, "the revocation is sent");

    // A public base does not reach a contact whose override says no...
    wb.set_own_field_public(&field_id).unwrap();
    wb.run_owed_repropagation().unwrap();
    assert_eq!(
        pending(&wb),
        after_add + 2,
        "the override still hides the field, so nothing changes for the contact"
    );

    // ...until the override is cleared: then the field is visible again.
    wb.remove_contact_visibility_override(&friend_id, &field_id)
        .unwrap();
    wb.run_owed_repropagation().unwrap();
    assert_eq!(
        pending(&wb),
        after_add + 3,
        "clearing the override changes what the contact sees, so it is sent"
    );
}

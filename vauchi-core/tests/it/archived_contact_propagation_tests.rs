// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Archived contacts receive no card updates and catch up on unarchive
//! (`2026-07-05-ungrouped-contacts-default-open` Phase 2b, owner decision
//! 2026-07-10: "all contacts" excludes blocked AND archived).

use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

/// Identity with one Visible field and one ratcheted exchanged contact.
fn world_with_visible_field() -> (Vauchi, String, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let field = ContactField::new(FieldType::Email, "Work", "o@co.example", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();
    wb.set_own_field_public(&field_id).unwrap();

    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange([9u8; 32], ContactCard::new("Bob"), shared.clone(), 0);
    let bob = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    let their_dh = vauchi_core::exchange::X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&bob, &shared, *their_dh.public_key())
        .unwrap();
    (wb, bob, field_id)
}

// @scenario: contact_management :: Archive a contact
#[test]
fn archived_contact_receives_no_card_updates() {
    let (wb, bob, field_id) = world_with_visible_field();
    wb.archive_contact(&bob).unwrap();

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&field_id, "new@co.example", 1)
        .unwrap();
    wb.update_own_card(&new_card).unwrap();

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 0, "an archived contact must receive nothing");
    assert_eq!(
        wb.storage()
            .pending()
            .get_pending_updates(&bob)
            .unwrap()
            .len(),
        0,
        "no pending update may target the archived contact"
    );
}

// @scenario: contact_management :: Archive a contact
#[test]
fn unarchive_queues_catch_up_with_current_card() {
    let (wb, bob, field_id) = world_with_visible_field();
    wb.archive_contact(&bob).unwrap();

    // The card changes while Bob is archived — he must not see it yet.
    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&field_id, "new@co.example", 1)
        .unwrap();
    wb.update_own_card(&new_card).unwrap();
    wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(
        wb.storage()
            .pending()
            .get_pending_updates(&bob)
            .unwrap()
            .len(),
        0,
        "precondition: nothing queued while archived"
    );

    wb.unarchive_contact(&bob).unwrap();
    assert!(
        !wb.storage()
            .pending()
            .get_pending_updates(&bob)
            .unwrap()
            .is_empty(),
        "unarchive must queue a catch-up carrying the current card"
    );
}

// @scenario: contact_management :: Archive a contact
#[test]
fn unarchive_of_never_ratcheted_contact_stays_graceful() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let contact = Contact::from_exchange(
        [8u8; 32],
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
        0,
    );
    let carol = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    wb.archive_contact(&carol).unwrap();
    wb.unarchive_contact(&carol)
        .expect("unarchive must not fail when no catch-up can be encrypted");
}

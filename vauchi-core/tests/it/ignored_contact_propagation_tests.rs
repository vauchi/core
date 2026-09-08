// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ignore is one-way and non-destructive (ADR-072): our updates keep
//! flowing to an ignored contact, and theirs keep being applied. This
//! pins that ignore never reaches the propagation or receive gates that
//! archive and block do.

use crate::sync_card_update_tests::{create_valid_update, setup_exchange_with_ratchets};
use vauchi_core::api::process_single_card_update;
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

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignored_contact_still_receives_our_card_updates() {
    let (wb, bob, field_id) = world_with_visible_field();
    wb.ignore_contact(&bob).unwrap();

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&field_id, "new@co.example", 1)
        .unwrap();
    wb.update_own_card(&new_card).unwrap();

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1, "an ignored contact must still be queued");
    assert_eq!(
        wb.storage()
            .pending()
            .get_pending_updates(&bob)
            .unwrap()
            .len(),
        1,
        "exactly one pending update must target the ignored contact"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignored_contacts_updates_are_still_applied_to_their_card() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");
    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    alice_wb.ignore_contact(&bob_contact_id).unwrap();
    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    )
    .expect("an ignored contact's update must be accepted");

    let bob = alice_wb.get_contact(&bob_contact_id).unwrap().unwrap();
    assert_eq!(bob.card().display_name(), "Bob Updated");
    assert!(bob.is_ignored(), "applying their update must not un-ignore");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card revocation propagation (fix direction C of
//! `2026-06-08-card-revocation-not-propagated`).
//!
//! `repropagate_to_contact` historically diffed `empty_card -> own_card`
//! (add-only), so revoking a field's visibility queued no `Removed` delta and
//! the peer kept the value forever. The fix tracks the per-contact last-sent
//! visible field-id set and diffs it against the new effective set, emitting
//! `Removed` for fields that dropped out of visibility.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi,
    exchange::X3DHKeyPair,
};

fn create_test_vauchi() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn add_contact_with_ratchet(wb: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared, *their_dh.public_key())
        .unwrap();
    contact_id
}

fn own_field_id(wb: &Vauchi, label: &str) -> String {
    wb.own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap()
        .id()
        .to_string()
}

// @scenario: visibility_control :: Revoking the last visible field notifies the contact
#[test]
fn revoking_last_visible_field_queues_a_removal_update() {
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&wb, "work");
    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let label = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();

    // Grant: Bob joins Work and receives `work`. Establishes the last-sent
    // baseline ({work}).
    wb.add_contact_to_group_and_repropagate(label.id(), &bob_id)
        .unwrap();
    let after_grant = wb.storage().get_pending_updates(&bob_id).unwrap().len();
    assert!(
        after_grant >= 1,
        "Granting `work` should queue an add update"
    );

    // Revoke: drop `work` from the group. Bob now sees nothing — but he must
    // be told to REMOVE `work`, not silently left with the stale value.
    wb.set_group_field_visibility_and_repropagate(label.id(), &work, false)
        .unwrap();
    let after_revoke = wb.storage().get_pending_updates(&bob_id).unwrap().len();

    assert!(
        after_revoke > after_grant,
        "Revoking the last visible field must queue a removal update \
         (was {after_grant}, now {after_revoke})"
    );
}

// @scenario: visibility_control :: Revoking an exchange-shared field notifies the contact
#[test]
fn revoking_exchange_shared_field_as_first_repropagation_emits_removal() {
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&wb, "work");
    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let label = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();

    // Simulate exchange completion: snapshot the shared baseline ({work})
    // WITHOUT any repropagation (mirrors persist_*_exchanged_contact).
    wb.initialize_sent_baseline(&bob_id).unwrap();
    let before = wb.storage().get_pending_updates(&bob_id).unwrap().len();

    // Revoke as the FIRST repropagation — the baseline must let it emit a
    // removal (without exchange-init this queued nothing).
    wb.remove_contact_from_group_and_repropagate(label.id(), &bob_id)
        .unwrap();
    let after = wb.storage().get_pending_updates(&bob_id).unwrap().len();

    assert!(
        after > before,
        "Revoking an exchange-shared field as the first repropagation must \
         queue a removal (was {before}, now {after})"
    );
}

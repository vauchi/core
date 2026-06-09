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

use std::collections::HashSet;
use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi,
    api::process_single_card_update, crypto::cek::ContentEncryptionKey, exchange::X3DHKeyPair,
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
    let after_grant = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();
    assert!(
        after_grant >= 1,
        "Granting `work` should queue an add update"
    );

    // Revoke: drop `work` from the group. Bob now sees nothing — but he must
    // be told to REMOVE `work`, not silently left with the stale value.
    wb.set_group_field_visibility_and_repropagate(label.id(), &work, false)
        .unwrap();
    let after_revoke = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();

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
    let before = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();

    // Revoke as the FIRST repropagation — the baseline must let it emit a
    // removal (without exchange-init this queued nothing).
    wb.remove_contact_from_group_and_repropagate(label.id(), &bob_id)
        .unwrap();
    let after = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();

    assert!(
        after > before,
        "Revoking an exchange-shared field as the first repropagation must \
         queue a removal (was {before}, now {after})"
    );
}

// --- Two-recipient round-trip harness (G2/G3) ---
//
// The tests above assert the SEND side (a removal lands in the pending
// queue). G2/G3 assert the RECEIVE side: a second `Vauchi` instance applies
// the queued delta and its stored copy of the sharer's card actually loses
// (or keeps) the field. Sharer is the ratchet initiator; each recipient is
// the responder.

/// A recipient instance plus the contact ids that bridge it to the sharer.
struct Recipient {
    wb: Vauchi,
    /// The recipient's contact id *in the sharer's* storage (hex of the
    /// recipient's signing key) — what the sharer repropagates to.
    id_at_sharer: String,
    /// The sharer's contact id *in the recipient's* storage (hex of the
    /// sharer's signing key) — the card that receives updates.
    sharer_id_here: String,
    /// Pending-update ids already delivered, so each `deliver` only feeds the
    /// recipient's ratchet the newest message (keeps decrypt order trivial).
    delivered: HashSet<String>,
}

/// Pairs a fresh recipient with `sharer`: mutual contacts carrying each
/// other's signing key, a CEK on the sharer side (so `repropagate` takes the
/// CEK-wrapped path the receiver requires), and an initiator/responder
/// ratchet pair over a shared secret.
fn add_recipient(sharer: &Vauchi, sharer_pk: &[u8; 32], name: &str) -> Recipient {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    let recipient_pk = *wb.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();

    let mut at_sharer =
        Contact::from_exchange(recipient_pk, ContactCard::new(name), shared.clone(), 0);
    at_sharer.set_cek(ContentEncryptionKey::generate());
    let id_at_sharer = at_sharer.id().to_string();
    sharer.add_contact(at_sharer).unwrap();

    let sharer_here =
        Contact::from_exchange(*sharer_pk, ContactCard::new("Sharer"), shared.clone(), 0);
    let sharer_id_here = sharer_here.id().to_string();
    wb.add_contact(sharer_here).unwrap();

    let recipient_dh = X3DHKeyPair::generate();
    sharer
        .create_ratchet_as_initiator(&id_at_sharer, &shared, *recipient_dh.public_key())
        .unwrap();
    wb.create_ratchet_as_responder(&sharer_id_here, &shared, recipient_dh)
        .unwrap();

    Recipient {
        wb,
        id_at_sharer,
        sharer_id_here,
        delivered: HashSet::new(),
    }
}

/// Delivers every not-yet-seen pending update from `sharer` to `r`, applying
/// each through the real receive pipeline. Returns how many were delivered.
fn deliver(sharer: &Vauchi, r: &mut Recipient) -> usize {
    let pending = sharer
        .storage()
        .pending()
        .get_pending_updates(&r.id_at_sharer)
        .unwrap();
    let mut count = 0;
    for upd in pending {
        if !r.delivered.insert(upd.id.clone()) {
            continue;
        }
        process_single_card_update(
            r.wb.identity().unwrap(),
            r.wb.storage(),
            &r.sharer_id_here,
            &upd.payload,
        )
        .unwrap_or_else(|e| panic!("delivery failed: {e:?}"));
        count += 1;
    }
    count
}

/// Whether the recipient's stored copy of the sharer's card holds a field.
fn stored_card_has(r: &Recipient, label: &str) -> bool {
    r.wb.storage()
        .contacts()
        .load_contact(&r.sharer_id_here)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == label)
}

// @scenario: visibility_control :: Revoking propagates to the peer; an un-revoked peer keeps the field
#[test]
fn revoking_for_one_recipient_does_not_affect_another_recipient() {
    let alice = create_test_vauchi();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&alice, "work");
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    let group = alice.create_group("Work").unwrap();
    alice
        .set_group_field_visibility(group.id(), &work, true)
        .unwrap();

    let mut bob = add_recipient(&alice, &alice_pk, "Bob");
    let mut carol = add_recipient(&alice, &alice_pk, "Carol");

    // Grant `work` to both, deliver each grant.
    alice
        .add_contact_to_group_and_repropagate(group.id(), &bob.id_at_sharer)
        .unwrap();
    assert_eq!(deliver(&alice, &mut bob), 1, "Bob receives the grant");
    alice
        .add_contact_to_group_and_repropagate(group.id(), &carol.id_at_sharer)
        .unwrap();
    assert_eq!(deliver(&alice, &mut carol), 1, "Carol receives the grant");
    assert!(stored_card_has(&bob, "work"), "Bob has `work` after grant");
    assert!(
        stored_card_has(&carol, "work"),
        "Carol has `work` after grant"
    );

    // Revoke for Bob only (remove from the granting group), deliver.
    alice
        .remove_contact_from_group_and_repropagate(group.id(), &bob.id_at_sharer)
        .unwrap();
    assert_eq!(deliver(&alice, &mut bob), 1, "Bob receives the revocation");

    assert!(
        !stored_card_has(&bob, "work"),
        "Bob's stored card must drop `work` after revocation"
    );
    assert!(
        stored_card_has(&carol, "work"),
        "Carol (never revoked) must still hold `work`"
    );
}

// @scenario: visibility_control :: A field still granted by another group is not over-removed
#[test]
fn removing_from_one_group_keeps_a_field_granted_by_another() {
    let alice = create_test_vauchi();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&alice, "work");
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    let work_group = alice.create_group("Work").unwrap();
    let team_group = alice.create_group("Team").unwrap();
    alice
        .set_group_field_visibility(work_group.id(), &work, true)
        .unwrap();
    alice
        .set_group_field_visibility(team_group.id(), &work, true)
        .unwrap();

    let mut bob = add_recipient(&alice, &alice_pk, "Bob");

    // Bob is in both groups; deliver after each step so the ratchet sees one
    // new message at a time.
    alice
        .add_contact_to_group_and_repropagate(work_group.id(), &bob.id_at_sharer)
        .unwrap();
    deliver(&alice, &mut bob);
    alice
        .add_contact_to_group_and_repropagate(team_group.id(), &bob.id_at_sharer)
        .unwrap();
    deliver(&alice, &mut bob);
    assert!(stored_card_has(&bob, "work"), "Bob has `work` after grant");

    // Remove Bob from Work only — `work` is still granted via Team, so the
    // delivered card must not lose it (no over-removal).
    alice
        .remove_contact_from_group_and_repropagate(work_group.id(), &bob.id_at_sharer)
        .unwrap();
    deliver(&alice, &mut bob);

    assert!(
        stored_card_has(&bob, "work"),
        "Bob must keep `work` — still granted by the Team group"
    );
}

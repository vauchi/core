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

use crate::common::two_recipient::{
    add_recipient, add_recipient_no_cek, deliver, stored_card_display_name, stored_card_has,
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

    // Make `work` group-only: remove it from the public base so the Work group
    // is its sole grant and leaving it revokes `work` (ADR-054 D3 — an ungrouped
    // contact otherwise keeps public-base fields). Both Bob and Carol are still
    // ungrouped, so this adds no `work` delta; drain any baseline send.
    alice.set_own_field_private(&work).unwrap();
    deliver(&alice, &mut bob);

    // Grant `work` to both via the group, deliver each grant.
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

// @scenario: visibility_control :: Removing a field from the public base revokes it from an ungrouped contact
#[test]
fn set_own_field_private_revokes_a_public_field_from_an_ungrouped_contact() {
    let alice = create_test_vauchi();
    alice
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&alice, "work");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    // Fields default hidden (field-centric model) — toggle Visible so Bob
    // receives the initial share that the revocation below takes back.
    alice.set_own_field_public(&work).unwrap();

    // Bob exchanges and stays UNGROUPED → he sees the Visible-toggled field.
    // Deliver the initial share (the toggle armed the marker).
    let mut bob = add_recipient(&alice, &alice_pk, "Bob");
    alice.run_owed_repropagation().unwrap();
    assert_eq!(
        deliver(&alice, &mut bob),
        1,
        "Bob receives the initial public field"
    );
    assert!(
        stored_card_has(&bob, "work"),
        "Bob holds `work` from the public base"
    );

    // Remove `work` from the public base. Before the marker wiring this armed
    // nothing and Bob kept the field forever; now it triggers a Removed delta.
    alice.set_own_field_private(&work).unwrap();
    alice.run_owed_repropagation().unwrap();
    assert_eq!(
        deliver(&alice, &mut bob),
        1,
        "Bob receives the public-base revocation"
    );

    assert!(
        !stored_card_has(&bob, "work"),
        "set_own_field_private revokes `work` from the ungrouped contact's stored card"
    );
}

// A freshly-exchanged contact has NO CEK (`Contact::from_exchange` sets
// cek=None). The mobile first-send `repropagate_to_contact` must still ship a
// CEK-wrapped (v0x02) payload — the receiver rejects a raw delta as
// `bad_payload`. Before the fix, repropagate CEK-wrapped only
// `if contact.cek().is_some()`, so the first device-to-device card update was
// rejected. The default `add_recipient` masks this by pre-seeding a CEK; this
// test uses `add_recipient_no_cek`
// (2026-06-29-card-update-duplicate-message-paths: CEK-less first send).
// @scenario: sync_updates :: First update to a freshly-exchanged (cek-less) contact is accepted
#[test]
fn first_repropagate_to_cek_less_contact_is_accepted() {
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

    // Freshly-exchanged contact: NO CEK.
    let mut bob = add_recipient_no_cek(&alice, &alice_pk, "Bob");

    // First repropagate to bob (grant `work` via the group). The receiver only
    // accepts CEK-wrapped payloads, so a raw delta to this cek-less contact is
    // rejected — `deliver` panics on the resulting bad_payload before the fix.
    alice
        .add_contact_to_group_and_repropagate(group.id(), &bob.id_at_sharer)
        .unwrap();
    assert_eq!(
        deliver(&alice, &mut bob),
        1,
        "the first update to a cek-less freshly-exchanged contact must be accepted"
    );
    assert!(
        stored_card_has(&bob, "work"),
        "the granted field must apply at the freshly-exchanged peer"
    );
}

// A display-name change must repropagate to contacts. Before the fix,
// `update_display_name` did not mark the own card for repropagation, AND the
// repropagate baseline was stamped with the current name so the diff emitted no
// `DisplayNameChanged` — so a rename never reached contacts (they kept the
// exchange-time name forever). The fix marks repropagation on rename and tracks
// `last_sent_display_name` per contact so the baseline carries the last-sent
// name. (2026-06-29 device test: "renamed but it did not sync".)
// @scenario: sync_updates :: A display-name change propagates to contacts
#[test]
fn renaming_own_card_propagates_the_new_name_to_contacts() {
    let mut alice = create_test_vauchi(); // identity "Alice"
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let mut bob = add_recipient_no_cek(&alice, &alice_pk, "Bob");

    alice.update_display_name("Alice Renamed").unwrap();
    alice.run_owed_repropagation().unwrap();

    assert!(
        deliver(&alice, &mut bob) >= 1,
        "a display-name change must repropagate to the contact"
    );
    assert_eq!(
        stored_card_display_name(&bob),
        "Alice Renamed",
        "the contact's stored card must adopt the renamed display name"
    );
}

// Re-propagation is the owed-convergence fallback, but it historically shipped
// the `CardDelta::compute` default version (1) on every send. Receivers floor
// on the last version they applied (#42), so once any send path advances a
// receiver's floor to 2, every re-propagation is rejected as StaleVersion —
// silently breaking hide/revoke/rename convergence. Each re-propagation must
// stamp and record an incrementing per-contact sent version, like the normal
// propagation path does.
// @scenario: sync_updates :: Re-propagated deltas carry incrementing versions
#[test]
fn consecutive_repropagations_stamp_incrementing_delta_versions() {
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&wb, "work");
    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let label = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();

    assert_eq!(
        wb.storage()
            .contacts()
            .last_sent_delta_version(&bob_id)
            .unwrap(),
        0,
        "nothing sent yet — no version recorded"
    );

    // First re-propagation (granting `work`) stamps version 1.
    wb.add_contact_to_group_and_repropagate(label.id(), &bob_id)
        .unwrap();
    assert_eq!(
        wb.storage()
            .contacts()
            .last_sent_delta_version(&bob_id)
            .unwrap(),
        1,
        "the first re-propagation must record sent version 1"
    );

    // Second re-propagation (revoking `work`) stamps version 2 — a receiver
    // whose floor is already 1 must not reject it as stale.
    wb.remove_contact_from_group_and_repropagate(label.id(), &bob_id)
        .unwrap();
    assert_eq!(
        wb.storage()
            .contacts()
            .last_sent_delta_version(&bob_id)
            .unwrap(),
        2,
        "the second re-propagation must record sent version 2"
    );
}

// The `delta.is_empty()` early return sends nothing, so it must not bump the
// sent version — otherwise gaps make the recorded floor lie about what
// receivers actually applied.
// @scenario: sync_updates :: Re-propagated deltas carry incrementing versions
#[test]
fn empty_repropagation_does_not_bump_the_sent_version() {
    let wb = create_test_vauchi();
    let carol_id = add_contact_with_ratchet(&wb, "Carol");
    let label = wb.create_group("Empty").unwrap();

    // First re-propagation carries only the display name (the group grants no
    // fields): it is a real send and records version 1.
    wb.add_contact_to_group_and_repropagate(label.id(), &carol_id)
        .unwrap();
    assert_eq!(
        wb.storage()
            .contacts()
            .last_sent_delta_version(&carol_id)
            .unwrap(),
        1,
        "the first re-propagation must record sent version 1"
    );

    // Removing Carol leaves nothing visible and nothing revoked: the delta is
    // empty, nothing is queued, and the sent version must not move.
    wb.remove_contact_from_group_and_repropagate(label.id(), &carol_id)
        .unwrap();
    assert_eq!(
        wb.storage()
            .contacts()
            .last_sent_delta_version(&carol_id)
            .unwrap(),
        1,
        "an empty re-propagation sends nothing and must not bump the version"
    );
}

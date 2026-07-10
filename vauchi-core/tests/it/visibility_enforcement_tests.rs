// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Enforcement Tests
//!
//! Verifies that visibility rules are actually enforced at the propagation
//! layer — not just that rules can be set/queried, but that hidden fields
//! are excluded from deltas sent to contacts.
//!
//! Cross-reference: Tracker #201, #202, #203

use std::collections::HashSet;

use vauchi_core::contact::{FieldVisibility, VisibilityRules};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{CardDelta, FieldChange};
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi};

use crate::common::two_recipient::{add_recipient, deliver, stored_card_has};

// -- Helpers -----------------------------------------------------------

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

fn add_contact_with_ratchet_and_visibility(
    wb: &Vauchi,
    name: &str,
    setup_rules: impl FnOnce(&mut VisibilityRules),
) -> String {
    let identity = Identity::create(name, 0);
    let shared = SymmetricKey::generate();
    let mut contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();

    setup_rules(contact.visibility_rules_mut().unwrap());
    wb.add_contact(contact).unwrap();

    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared, *their_dh.public_key())
        .unwrap();

    contact_id
}

// -- Tests: Default Visibility -----------------------------------------

/// Tracker #201: Verify that new fields are visible to everyone by default.
/// This is the current design — new fields default to `Everyone`.
// @scenario: visibility_control :: New fields default to visible to all contacts
#[test]
fn test_default_visibility_is_everyone() {
    let rules = VisibilityRules::new();

    // Unknown field IDs default to Everyone
    assert_eq!(*rules.get("nonexistent_field"), FieldVisibility::Everyone);
    assert!(rules.can_see("nonexistent_field", "any_contact"));
}

/// A new field propagates to nobody until its Visible toggle is set, then
/// reaches every contact alike (field-centric model, 2026-07-10).
// @scenario: visibility_control :: New fields default to hidden for ungrouped contacts
#[test]
fn test_new_field_propagates_only_after_visible_toggle() {
    let wb = create_test_vauchi();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let carol_id = add_contact_with_ratchet(&wb, "Carol");

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let field = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
    let field_id = field.id().to_string();
    let _ = new_card.add_field(field);

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 0, "an untoggled new field reaches nobody");

    new_card.set_field_shown(&field_id, true);
    wb.update_own_card(&new_card).unwrap();

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 2, "a Visible-toggled field reaches every contact");

    let bob_pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    let carol_pending = wb
        .storage()
        .pending()
        .get_pending_updates(&carol_id)
        .unwrap();
    assert_eq!(bob_pending.len(), 1);
    assert_eq!(carol_pending.len(), 1);
}

// -- Tests: Contacts(allowed_set) Enforcement --------------------------

/// Tracker #202: Verify that `Contacts(allowed_set)` is enforced at
/// propagation time — contact in the set sees the field, others don't.
// @scenario: visibility_control :: Show a field only to specific contacts
#[test]
fn test_contacts_variant_enforced_at_propagation() {
    let wb = create_test_vauchi();

    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com", 0);
    let email_id = email_field.id().to_string();

    // Add Bob (will be in the allowed set) and Carol (will not be)
    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let carol_id = add_contact_with_ratchet_and_visibility(&wb, "Carol", |rules| {
        // Carol's visibility rules: email visible only to Bob (not Carol)
        // Note: visibility rules on a Contact control what that contact can see
        let mut allowed = HashSet::new();
        allowed.insert("nobody_relevant".to_string());
        rules.set_contacts(&email_id, allowed);
    });

    // Bob's per-contact rules: default (no restriction). The own-card toggle
    // must still be Visible for anything to propagate (field-centric model).

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(email_field);
    new_card.set_field_shown(&email_id, true);
    wb.update_own_card(&new_card).unwrap();

    let _queued = wb.propagate_card_update(&old_card, &new_card).unwrap();

    // Bob should get the update (toggle Visible, no per-contact restriction)
    let bob_pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert_eq!(
        bob_pending.len(),
        1,
        "Bob should receive update (default Everyone visibility)"
    );

    // Carol should NOT get the update (email restricted, not in allowed set)
    let carol_pending = wb
        .storage()
        .pending()
        .get_pending_updates(&carol_id)
        .unwrap();
    assert_eq!(
        carol_pending.len(),
        0,
        "Carol should not receive update (not in allowed contacts set)"
    );
}

// -- Tests: Display Name Passthrough -----------------------------------

/// Tracker #203: Display name changes must always pass through visibility
/// filtering — even when all fields are set to Nobody.
// @scenario: visibility_control :: Encrypted updates reveal nothing about hidden fields
#[test]
fn test_display_name_change_passes_through_nobody_rules() {
    let mut rules = VisibilityRules::new();
    rules.set_nobody("field_1");
    rules.set_nobody("field_2");

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new, 0);
    let filtered = delta.filter_for_contact("bob", &rules);

    assert!(
        !filtered.is_empty(),
        "Display name change must pass through visibility filter"
    );
    assert!(filtered.changes.iter().any(|c| matches!(
        c,
        FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"
    )));
}

/// Display name change propagates even when a hidden field is also changed.
// @scenario: visibility_control :: Encrypted updates reveal nothing about hidden fields
#[test]
fn test_display_name_propagated_alongside_hidden_field() {
    let wb = create_test_vauchi();

    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com", 0);
    let email_id = email_field.id().to_string();

    // Bob's visibility rules: email hidden (Nobody)
    let _bob_id = add_contact_with_ratchet_and_visibility(&wb, "Bob", |rules| {
        rules.set_nobody(&email_id);
    });

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.set_display_name("Alice Updated");
    let _ = new_card.add_field(email_field);

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();

    // Should still propagate because display name changed (even though email is hidden)
    assert_eq!(
        queued, 1,
        "Display name change should cause propagation even with hidden field"
    );
}

// -- Tests: Visibility Rule Removal ------------------------------------

/// Removing a visibility rule should revert the field to Everyone,
/// causing it to be included in subsequent propagation.
// @scenario: visibility_control :: New fields default to visible to all contacts
#[test]
fn test_remove_rule_reverts_to_everyone_at_delta_level() {
    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com", 0);
    let email_id = email_field.id().to_string();

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(email_field);

    let delta = CardDelta::compute(&old, &new, 0);

    let mut rules = VisibilityRules::new();
    rules.set_nobody(&email_id);

    let filtered_hidden = delta.filter_for_contact("bob", &rules);
    assert!(
        filtered_hidden.is_empty(),
        "Field should be hidden with Nobody rule"
    );

    // Remove rule — reverts to Everyone
    rules.remove(&email_id);

    let filtered_visible = delta.filter_for_contact("bob", &rules);
    assert!(
        !filtered_visible.is_empty(),
        "Field should be visible after removing Nobody rule"
    );
    assert!(
        filtered_visible
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Added { .. }))
    );
}

// -- Tests: Nobody Enforcement -----------------------------------------

/// Nobody visibility must exclude field for all contacts, not just one.
// @scenario: visibility_control :: Make a field private (visible to none)
#[test]
fn test_nobody_excludes_from_all_contacts() {
    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com", 0);
    let email_id = email_field.id().to_string();

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(email_field);

    let delta = CardDelta::compute(&old, &new, 0);

    let mut rules = VisibilityRules::new();
    rules.set_nobody(&email_id);

    // Test with multiple different contact IDs
    for contact in &["bob", "carol", "dave", "eve", "mallory"] {
        let filtered = delta.filter_for_contact(contact, &rules);
        assert!(
            filtered.is_empty(),
            "Nobody rule should hide field from '{}' but didn't",
            contact
        );
    }
}

/// Full integration: mixed visibility — some contacts see some fields,
/// others don't, display name always passes.
// @scenario: visibility_control :: Hide a field from a specific contact
// @scenario: visibility_control :: Encrypted updates reveal nothing about hidden fields
// @scenario: contacts_management :: Contact shows only fields I can see
#[test]
fn test_mixed_visibility_propagation() {
    let wb = create_test_vauchi();

    let email_field = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
    let email_id = email_field.id().to_string();
    let phone_field = ContactField::new(FieldType::Phone, "personal", "+1234567890", 0);
    let phone_id = phone_field.id().to_string();

    // Bob: email hidden, phone visible (default)
    let bob_id = add_contact_with_ratchet_and_visibility(&wb, "Bob", |rules| {
        rules.set_nobody(&email_id);
    });

    // Carol: phone hidden, email visible (default)
    let carol_id = add_contact_with_ratchet_and_visibility(&wb, "Carol", |rules| {
        rules.set_nobody(&phone_id);
    });

    let dave_id = add_contact_with_ratchet_and_visibility(&wb, "Dave", |rules| {
        rules.set_nobody(&email_id);
        rules.set_nobody(&phone_id);
    });

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.set_display_name("Alice Updated");
    let _ = new_card.add_field(email_field);
    let _ = new_card.add_field(phone_field);

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();

    // All 3 contacts should get an update (display name changed for all)
    assert_eq!(
        queued, 3,
        "All contacts should get update due to display name change"
    );

    // Bob gets phone + display name (not email)
    let bob_pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert_eq!(bob_pending.len(), 1);

    // Carol gets email + display name (not phone)
    let carol_pending = wb
        .storage()
        .pending()
        .get_pending_updates(&carol_id)
        .unwrap();
    assert_eq!(carol_pending.len(), 1);

    // Dave gets display name only (both fields hidden)
    let dave_pending = wb
        .storage()
        .pending()
        .get_pending_updates(&dave_id)
        .unwrap();
    assert_eq!(dave_pending.len(), 1);
}

// -- Tests: Group-aware content-edit propagation (ADR-054 G4) -----------

// ADR-054 G4 (2026-06-08-sync-card-update-not-group-filtered): an ordinary
// own-card edit must propagate through the group-aware resolver, not the
// Layer-A-only filter. A grouped recipient whose group does not grant the new
// field must not receive it (both recipients grouped → the default-closed gate
// excludes B; no Layer-A rule needed).
// @scenario: visibility_control :: A content edit reaches only the granting group
#[test]
fn content_edit_reaches_only_the_granting_group() {
    let wb = create_test_vauchi();
    let wb_pk = *wb.identity().unwrap().signing_public_key();

    let work = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
    let work_id = work.id().to_string();

    let granting = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(granting.id(), &work_id, true)
        .unwrap();
    let other = wb.create_group("Friends").unwrap();

    let mut a = add_recipient(&wb, &wb_pk, "A");
    let mut b = add_recipient(&wb, &wb_pk, "B");
    wb.add_contact_to_group(granting.id(), &a.id_at_sharer)
        .unwrap();
    wb.add_contact_to_group(other.id(), &b.id_at_sharer)
        .unwrap();

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(work);

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(
        queued, 1,
        "Only A (in the granting group) is queued; B's delta is empty (skipped)"
    );
    assert!(
        wb.storage()
            .pending()
            .get_pending_updates(&b.id_at_sharer)
            .unwrap()
            .is_empty(),
        "B (group does not grant `work`) has no queued update"
    );

    deliver(&wb, &mut a);
    deliver(&wb, &mut b);
    assert!(
        stored_card_has(&a, "work"),
        "A's group grants `work` → A receives it"
    );
    assert!(
        !stored_card_has(&b, "work"),
        "B's group does not grant `work` → B does not receive it"
    );
}

// Field-centric partition (2026-07-10): a group-assigned field reaches only
// group members, while a Visible-toggled unassigned field reaches every
// contact alike — grouped and ungrouped. Brackets both audience changes so
// neither direction regresses.
// @scenario: visibility_control :: A content edit reaches ungrouped contacts via the public base card
#[test]
fn content_edit_partitions_group_assigned_and_toggled_fields() {
    let wb = create_test_vauchi();
    let wb_pk = *wb.identity().unwrap().signing_public_key();

    let work = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
    let work_id = work.id().to_string();
    let personal = ContactField::new(FieldType::Phone, "personal", "+15550000", 0);
    let personal_id = personal.id().to_string();

    // `work` is granted to the Work group (group-assigned); `personal` is in
    // no group and gets the Visible toggle.
    let group = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(group.id(), &work_id, true)
        .unwrap();

    let mut grouped = add_recipient(&wb, &wb_pk, "Grouped");
    let mut ungrouped = add_recipient(&wb, &wb_pk, "Ungrouped");
    wb.add_contact_to_group(group.id(), &grouped.id_at_sharer)
        .unwrap();

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(work);
    let _ = new_card.add_field(personal);
    new_card.set_field_shown(&personal_id, true);
    wb.update_own_card(&new_card).unwrap();

    wb.propagate_card_update(&old_card, &new_card).unwrap();
    deliver(&wb, &mut grouped);
    deliver(&wb, &mut ungrouped);

    // Grouped recipient: the group's union (`work`) plus the
    // Visible-toggled unassigned `personal` — the toggle applies to
    // every contact alike.
    assert!(
        stored_card_has(&grouped, "work"),
        "Grouped recipient's group grants `work`"
    );
    assert!(
        stored_card_has(&grouped, "personal"),
        "Visible toggle reaches group members too"
    );

    // Ungrouped recipient: only the Visible-toggled `personal`; the
    // group-assigned `work` is closed to non-members.
    assert!(
        !stored_card_has(&ungrouped, "work"),
        "group-assigned `work` is closed to a contact outside the group"
    );
    assert!(
        stored_card_has(&ungrouped, "personal"),
        "ungrouped recipient receives the Visible-toggled `personal`"
    );
}

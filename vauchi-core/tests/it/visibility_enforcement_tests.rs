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

/// Verify that a new field added to the card gets propagated to all contacts
/// (because default visibility is Everyone).
// @scenario: visibility_control :: New contacts see default-visible fields
#[test]
fn test_new_field_propagated_to_all_contacts_by_default() {
    let wb = create_test_vauchi();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let carol_id = add_contact_with_ratchet(&wb, "Carol");

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ));

    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 2, "New field should be propagated to both contacts");

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

    // Bob's visibility rules: default (Everyone) — Bob can see everything

    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(email_field);

    let _queued = wb.propagate_card_update(&old_card, &new_card).unwrap();

    // Bob should get the update (default visibility = Everyone)
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

// ADR-054 D3: a content edit must reach an *ungrouped* recipient via the
// Layer-A public base card (not hidden), while a *grouped* recipient receives
// only their group's union. Brackets the over-restrict direction so the G4 fix
// cannot hide public fields from ungrouped contacts.
// @scenario: visibility_control :: A content edit reaches ungrouped contacts via the public base card
#[test]
fn content_edit_ungrouped_recipient_gets_public_base() {
    let wb = create_test_vauchi();
    let wb_pk = *wb.identity().unwrap().signing_public_key();

    let work = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
    let work_id = work.id().to_string();
    let personal = ContactField::new(FieldType::Phone, "personal", "+15550000", 0);

    // `work` is granted to the Work group; `personal` is granted by no group,
    // so it stays Layer-A `Everyone` (public base).
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

    wb.propagate_card_update(&old_card, &new_card).unwrap();
    deliver(&wb, &mut grouped);
    deliver(&wb, &mut ungrouped);

    // Grouped recipient: only the group's union (`work`), not the ungranted
    // `personal`.
    assert!(
        stored_card_has(&grouped, "work"),
        "Grouped recipient's group grants `work`"
    );
    assert!(
        !stored_card_has(&grouped, "personal"),
        "Grouped recipient is default-closed: `personal` (no group) is hidden"
    );

    // Ungrouped recipient: the Layer-A public base card — both `Everyone` fields.
    assert!(
        stored_card_has(&ungrouped, "work"),
        "D3: ungrouped recipient falls back to Layer-A; `work` is Everyone"
    );
    assert!(
        stored_card_has(&ungrouped, "personal"),
        "D3: ungrouped recipient receives the public base `personal`"
    );
}

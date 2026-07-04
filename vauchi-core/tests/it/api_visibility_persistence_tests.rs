// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact visibility persistence.
//!
//! Verifies that toggle_field_visibility persists changes
//! and get_effective_field_visibility reflects them.

use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

/// Helper: create Vauchi with identity, own card with fields,
/// and an exchanged contact.
fn setup_with_fields() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "Work Email",
        "alice@example.com",
        0,
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-0000",
        0,
    ))
    .unwrap();

    // Create exchanged contact (has visibility rules)
    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), 0);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

/// Resolves an own-card field label to its generated id. Reads via the id,
/// not the label, must match how every production path resolves visibility
/// (`is_field_visible_by_label` → `own_field_id_by_label` → id-keyed rules).
/// A test that reads by label would pass even when the toggle writes the
/// wrong key (F1, 2026-06-14 visibility layering).
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

// @scenario: navigation.feature - Toggle visibility persists
#[test]
fn test_toggle_visibility_persists() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = own_field_id(&wb, "Work Email");

    // Default: all fields visible
    let visible = wb
        .get_effective_field_visibility(&contact_id, &email_id)
        .unwrap();
    assert!(visible, "Fields should be visible by default");

    let new_state = wb
        .toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!new_state, "Toggle should return new state (hidden)");

    let visible_after = wb
        .get_effective_field_visibility(&contact_id, &email_id)
        .unwrap();
    assert!(!visible_after, "Visibility must persist after toggle");
}

// @scenario: navigation.feature - Toggle visibility is per-field
#[test]
fn test_toggle_visibility_per_field() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = own_field_id(&wb, "Work Email");
    let phone_id = own_field_id(&wb, "Mobile");

    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();

    // Email hidden, phone still visible
    let email_vis = wb
        .get_effective_field_visibility(&contact_id, &email_id)
        .unwrap();
    let phone_vis = wb
        .get_effective_field_visibility(&contact_id, &phone_id)
        .unwrap();
    assert!(!email_vis, "Email should be hidden");
    assert!(phone_vis, "Phone should remain visible");
}

// @scenario: navigation.feature - Toggle visibility twice restores
#[test]
fn test_toggle_twice_restores_visibility() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = own_field_id(&wb, "Work Email");

    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();

    let visible = wb
        .get_effective_field_visibility(&contact_id, &email_id)
        .unwrap();
    assert!(visible, "Double toggle should restore visibility");
}

// A toggle is a per-contact **override** (Layer C): it must win over a
// group grant and persist when the contact leaves the group — parity with
// `set_field_private` (2026-06-14 visibility layering, F3). Before the fix
// the toggle wrote Layer-A `visibility_rules`, which the D3 gate skips for a
// grouped contact, so the toggle was a no-op here.
// @internal
#[test]
fn test_toggle_hides_grouped_contact_via_override() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = own_field_id(&wb, "Work Email");

    let group = wb.create_group("Team").unwrap();
    wb.set_group_field_visibility(group.id(), &email_id, true)
        .unwrap();
    wb.add_contact_to_group(group.id(), &contact_id).unwrap();
    assert!(
        wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "the group grant makes the field visible"
    );

    let now_visible = wb
        .toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!now_visible, "toggle returns hidden for a grouped contact");
    assert!(
        !wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "toggle must hide the field via an override even when a group grants it"
    );

    wb.remove_contact_from_group(group.id(), &contact_id)
        .unwrap();
    assert!(
        !wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "the override persists when the contact leaves the group"
    );
}

// ============================================================
// Public base (Layer A) — `set_own_field_public/private` shapes the
// own card's per-field `field_visibility`, the subset an *ungrouped*
// contact sees (2026-06-14 visibility layering).
// ============================================================

// @internal
#[test]
fn set_own_field_private_hides_field_from_ungrouped_contact() {
    let (wb, contact_id) = setup_with_fields();
    let card = wb.own_card().unwrap().unwrap();
    let email_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Work Email")
        .unwrap()
        .id()
        .to_string();
    let phone_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Mobile")
        .unwrap()
        .id()
        .to_string();

    // Default public base is `Everyone` → visible to an ungrouped contact.
    assert!(
        wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "default public base shows the field"
    );

    // Remove from the public base → hidden from the ungrouped contact.
    wb.set_own_field_private(&email_id).unwrap();
    assert!(
        !wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "set_own_field_private must hide the field from an ungrouped contact"
    );
    // Per-field: the sibling stays in the public base.
    assert!(
        wb.get_effective_field_visibility(&contact_id, &phone_id)
            .unwrap(),
        "public-base change must be per-field"
    );

    // Restore to the public base.
    wb.set_own_field_public(&email_id).unwrap();
    assert!(
        wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "set_own_field_public must restore the field to the public base"
    );
}

// @internal
#[test]
fn set_own_field_private_persists_across_reload() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = wb
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == "Work Email")
        .unwrap()
        .id()
        .to_string();

    wb.set_own_field_private(&email_id).unwrap();

    let reloaded = wb.own_card().unwrap().unwrap();
    assert!(
        !reloaded.field_visibility().can_see(&email_id, &contact_id),
        "public-base private must persist on the own card"
    );
}

// @internal
#[test]
fn set_field_private_hides_grouped_contact_via_override_and_persists() {
    let (wb, contact_id) = setup_with_fields();
    let email_id = wb
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == "Work Email")
        .unwrap()
        .id()
        .to_string();

    // Grant the field via a group the contact is in.
    let group = wb.create_group("Team").unwrap();
    wb.set_group_field_visibility(group.id(), &email_id, true)
        .unwrap();
    wb.add_contact_to_group(group.id(), &contact_id).unwrap();
    assert!(
        wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "the group grant makes the field visible"
    );

    // A per-contact private must hide the field even for a *grouped* contact:
    // the Layer C override wins over the Layer B group grant (always-override
    // model — fixes the grouped-contact footgun).
    wb.set_field_private_and_repropagate(&contact_id, &email_id)
        .unwrap();
    assert!(
        !wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "set_field_private hides the field via a per-contact override even when a group grants it"
    );

    // The override is robust: it persists through group-membership changes.
    wb.remove_contact_from_group(group.id(), &contact_id)
        .unwrap();
    assert!(
        !wb.get_effective_field_visibility(&contact_id, &email_id)
            .unwrap(),
        "the per-contact override persists when the contact leaves the group"
    );
}

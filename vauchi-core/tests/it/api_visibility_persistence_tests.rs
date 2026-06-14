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

// @scenario: navigation.feature - Toggle visibility persists
#[test]
fn test_toggle_visibility_persists() {
    let (wb, contact_id) = setup_with_fields();

    // Default: all fields visible
    let visible = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(visible, "Fields should be visible by default");

    let new_state = wb
        .toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!new_state, "Toggle should return new state (hidden)");

    let visible_after = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!visible_after, "Visibility must persist after toggle");
}

// @scenario: navigation.feature - Toggle visibility is per-field
#[test]
fn test_toggle_visibility_per_field() {
    let (wb, contact_id) = setup_with_fields();

    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();

    // Email hidden, phone still visible
    let email_vis = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    let phone_vis = wb
        .get_effective_field_visibility(&contact_id, "Mobile")
        .unwrap();
    assert!(!email_vis, "Email should be hidden");
    assert!(phone_vis, "Phone should remain visible");
}

// @scenario: navigation.feature - Toggle visibility twice restores
#[test]
fn test_toggle_twice_restores_visibility() {
    let (wb, contact_id) = setup_with_fields();

    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();

    let visible = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(visible, "Double toggle should restore visibility");
}

// ============================================================
// Label-keyed Layer-A visibility (PAE G3 push-down — was inline
// in vauchi-platform's Hide/Show/IsFieldVisibleToContact arms)
// ============================================================

// @internal
#[test]
fn set_field_visibility_by_label_hides_and_shows_exact_field() {
    let (wb, contact_id) = setup_with_fields();

    assert!(
        wb.is_field_visible_by_label(&contact_id, "Work Email")
            .unwrap()
    );

    wb.set_field_visibility_by_label(&contact_id, "Work Email", false)
        .unwrap();
    assert!(
        !wb.is_field_visible_by_label(&contact_id, "Work Email")
            .unwrap()
    );
    assert!(
        wb.is_field_visible_by_label(&contact_id, "Mobile").unwrap(),
        "hiding one field must not affect siblings"
    );

    wb.set_field_visibility_by_label(&contact_id, "Work Email", true)
        .unwrap();
    assert!(
        wb.is_field_visible_by_label(&contact_id, "Work Email")
            .unwrap()
    );
}

// @internal
#[test]
fn set_field_visibility_by_label_rejects_unknown_field() {
    let (wb, contact_id) = setup_with_fields();
    let err = wb
        .set_field_visibility_by_label(&contact_id, "No Such Label", false)
        .unwrap_err();
    assert!(
        err.to_string().contains("field"),
        "error must name the missing field: {err}"
    );
}

// @internal
#[test]
fn set_field_visibility_by_label_rejects_unknown_contact() {
    let (wb, _) = setup_with_fields();
    let err = wb
        .set_field_visibility_by_label("no-such-contact", "Work Email", false)
        .unwrap_err();
    assert!(
        err.to_string().contains("contact"),
        "error must name the missing contact: {err}"
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

// @internal
#[test]
fn is_field_visible_by_label_persists_across_reload() {
    let (wb, contact_id) = setup_with_fields();
    wb.set_field_visibility_by_label(&contact_id, "Mobile", false)
        .unwrap();

    let reloaded = wb.get_contact(&contact_id).unwrap().unwrap();
    let card = wb.own_card().unwrap().unwrap();
    let mobile_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Mobile")
        .unwrap()
        .id()
        .to_string();
    assert!(
        !reloaded
            .visibility_rules()
            .unwrap()
            .can_see(&mobile_id, &contact_id),
        "rule must be keyed by field id, not label"
    );
}

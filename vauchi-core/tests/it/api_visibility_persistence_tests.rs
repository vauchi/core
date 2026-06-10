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

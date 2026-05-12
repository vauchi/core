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

    // Add fields to own card
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
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate());
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

    // Toggle: hide the field
    let new_state = wb
        .toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!new_state, "Toggle should return new state (hidden)");

    // Verify persistence: read back
    let visible_after = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(!visible_after, "Visibility must persist after toggle");
}

// @scenario: navigation.feature - Toggle visibility is per-field
#[test]
fn test_toggle_visibility_per_field() {
    let (wb, contact_id) = setup_with_fields();

    // Hide email only
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

    // Toggle off
    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();
    // Toggle on
    wb.toggle_field_visibility(&contact_id, "Work Email")
        .unwrap();

    let visible = wb
        .get_effective_field_visibility(&contact_id, "Work Email")
        .unwrap();
    assert!(visible, "Double toggle should restore visibility");
}

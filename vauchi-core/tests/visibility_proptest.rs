// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-Based Tests for Visibility Resolution
//!
//! Uses proptest to verify invariants of the visibility model:
//! - No field leaks: hidden fields never appear in the visible set
//! - Ungrouped contacts see nothing in groups mode
//! - Deterministic: same inputs always produce same outputs
//! - Per-contact overrides take precedence over label visibility
//! - Multi-label union: contact in multiple labels sees union of fields
//!
//! Traces to: _private/features/visibility.feature @visibility @labels

mod common;

use std::collections::HashSet;

use proptest::prelude::*;

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{resolve_visible_fields, LabelManager};

use common::strategies::contact_id_strategy;

// ============================================================
// Strategies
// ============================================================

/// Strategy for generating a ContactCard with N email fields, returning
/// the card and the field IDs.
fn card_with_fields(num_fields: usize) -> (ContactCard, Vec<String>) {
    let mut card = ContactCard::new("Test User");
    let mut field_ids = Vec::new();

    for i in 0..num_fields {
        let field = ContactField::new(
            FieldType::Email,
            &format!("email_{}", i),
            &format!("user{}@example.com", i),
        );
        let fid = field.id().to_string();
        card.add_field(field).unwrap();
        field_ids.push(fid);
    }

    (card, field_ids)
}

// ============================================================
// No-group mode invariants
// ============================================================

proptest! {
    /// No field leaks in no-group mode: only shown fields are visible,
    /// and every shown field is visible.
    #[test]
    fn no_field_leak_no_groups(
        num_fields in 1..10usize,
        shown_mask in prop::collection::vec(any::<bool>(), 1..10),
    ) {
        let (mut card, field_ids) = card_with_fields(num_fields);

        // Mark fields as shown according to the mask
        for (i, fid) in field_ids.iter().enumerate() {
            if i < shown_mask.len() && shown_mask[i] {
                card.set_field_shown(fid, true);
            }
        }

        let label_manager = LabelManager::new();
        let visible = resolve_visible_fields(&card, &label_manager, "any-contact");

        // Every visible field must be in shown_fields
        for fid in &visible {
            prop_assert!(
                card.is_field_shown(fid),
                "Field {} is visible but not marked as shown",
                fid
            );
        }

        // Every shown field must be visible
        for fid in card.shown_fields() {
            prop_assert!(
                visible.contains(fid),
                "Field {} is shown but not in visible set",
                fid
            );
        }

        // Visible set equals shown_fields exactly
        prop_assert_eq!(&visible, card.shown_fields());
    }

    /// Fields not marked shown are never visible in no-group mode.
    #[test]
    fn hidden_fields_never_visible_no_groups(
        num_fields in 1..10usize,
    ) {
        let (card, field_ids) = card_with_fields(num_fields);
        // No fields marked shown (privacy-first default)

        let label_manager = LabelManager::new();
        let visible = resolve_visible_fields(&card, &label_manager, "contact-1");

        prop_assert!(
            visible.is_empty(),
            "No fields shown, but visible set is non-empty: {:?}",
            visible
        );

        // Verify each field individually
        for fid in &field_ids {
            prop_assert!(
                !visible.contains(fid),
                "Field {} should be hidden but appeared in visible set",
                fid
            );
        }
    }
}

// ============================================================
// Groups mode invariants
// ============================================================

proptest! {
    /// Ungrouped contacts see nothing in groups mode.
    #[test]
    fn ungrouped_contacts_see_nothing(
        num_groups in 1..5usize,
        num_fields in 1..10usize,
    ) {
        let (card, field_ids) = card_with_fields(num_fields);

        let mut label_manager = LabelManager::new();
        let mut label_ids = Vec::new();
        for i in 0..num_groups {
            let label = label_manager.create_label(&format!("Group_{}", i)).unwrap();
            let label_id = label.id().to_string();

            // Add some fields as visible to make labels non-trivial
            if !field_ids.is_empty() {
                let label_mut = label_manager.get_label_mut(&label_id).unwrap();
                label_mut.add_visible_field(&field_ids[0]);
            }
            label_ids.push(label_id);
        }

        // Contact not in any group
        let visible = resolve_visible_fields(&card, &label_manager, "ungrouped-contact");
        prop_assert!(
            visible.is_empty(),
            "Ungrouped contact should see no fields, saw {:?}",
            visible
        );
    }

    /// A contact in a label sees exactly the fields visible in that label
    /// (when in a single label with no overrides).
    #[test]
    fn single_label_visibility(
        num_fields in 2..8usize,
        visible_mask in prop::collection::vec(any::<bool>(), 2..8),
    ) {
        let (card, field_ids) = card_with_fields(num_fields);

        let mut label_manager = LabelManager::new();
        let label = label_manager.create_label("TestLabel").unwrap();
        let label_id = label.id().to_string();

        // Add contact to label
        label_manager.add_contact_to_label(&label_id, "bob").unwrap();

        // Set field visibility according to the mask
        let mut expected_visible: HashSet<String> = HashSet::new();
        let label_mut = label_manager.get_label_mut(&label_id).unwrap();
        for (i, fid) in field_ids.iter().enumerate() {
            if i < visible_mask.len() && visible_mask[i] {
                label_mut.add_visible_field(fid);
                expected_visible.insert(fid.clone());
            }
        }

        let visible = resolve_visible_fields(&card, &label_manager, "bob");
        prop_assert_eq!(
            visible,
            expected_visible,
            "Visible fields should match label's visible_fields"
        );
    }

    /// Multi-label union: a contact in multiple labels sees the union of
    /// all fields visible across those labels.
    #[test]
    fn multi_label_union(
        num_fields in 3..8usize,
        num_labels in 2..4usize,
    ) {
        let (card, field_ids) = card_with_fields(num_fields);

        let mut label_manager = LabelManager::new();
        let mut expected_visible: HashSet<String> = HashSet::new();

        for i in 0..num_labels {
            let label = label_manager.create_label(&format!("Label_{}", i)).unwrap();
            let label_id = label.id().to_string();

            // Add contact to each label
            label_manager.add_contact_to_label(&label_id, "carol").unwrap();

            // Each label shows one distinct field (if available)
            if i < field_ids.len() {
                let label_mut = label_manager.get_label_mut(&label_id).unwrap();
                label_mut.add_visible_field(&field_ids[i]);
                expected_visible.insert(field_ids[i].clone());
            }
        }

        let visible = resolve_visible_fields(&card, &label_manager, "carol");

        // Union property: every expected field is visible
        for fid in &expected_visible {
            prop_assert!(
                visible.contains(fid),
                "Field {} should be visible via label union but is missing",
                fid
            );
        }

        // No extra fields beyond the union
        for fid in &visible {
            prop_assert!(
                expected_visible.contains(fid),
                "Field {} is visible but not in any label's visible_fields",
                fid
            );
        }
    }
}

// ============================================================
// Per-contact override invariants
// ============================================================

proptest! {
    /// Per-contact override can hide a field that is visible via labels.
    #[test]
    fn override_hides_label_visible_field(
        num_fields in 1..8usize,
        override_index in 0..8usize,
    ) {
        let (card, field_ids) = card_with_fields(num_fields);
        let override_index = override_index % num_fields;

        let mut label_manager = LabelManager::new();
        let label = label_manager.create_label("AllVisible").unwrap();
        let label_id = label.id().to_string();

        label_manager.add_contact_to_label(&label_id, "dave").unwrap();

        // Make all fields visible in the label
        let label_mut = label_manager.get_label_mut(&label_id).unwrap();
        for fid in &field_ids {
            label_mut.add_visible_field(fid);
        }

        // Override: hide one specific field from dave
        let hidden_field = &field_ids[override_index];
        label_manager.set_contact_override("dave", hidden_field, false);

        let visible = resolve_visible_fields(&card, &label_manager, "dave");

        // The overridden field must NOT be visible
        prop_assert!(
            !visible.contains(hidden_field),
            "Field {} was hidden via override but still visible",
            hidden_field
        );

        // All other fields must still be visible
        for (i, fid) in field_ids.iter().enumerate() {
            if i != override_index {
                prop_assert!(
                    visible.contains(fid),
                    "Field {} should still be visible (not overridden)",
                    fid
                );
            }
        }
    }

    /// Per-contact override can grant visibility to a field not in any label.
    #[test]
    fn override_grants_visibility(
        num_fields in 2..8usize,
        grant_index in 0..8usize,
    ) {
        let (card, field_ids) = card_with_fields(num_fields);
        let grant_index = grant_index % num_fields;

        let mut label_manager = LabelManager::new();
        // Create a label but don't add any visible fields
        let label = label_manager.create_label("EmptyLabel").unwrap();
        let label_id = label.id().to_string();

        label_manager.add_contact_to_label(&label_id, "eve").unwrap();

        // Override: grant visibility to one field
        let granted_field = &field_ids[grant_index];
        label_manager.set_contact_override("eve", granted_field, true);

        let visible = resolve_visible_fields(&card, &label_manager, "eve");

        // The overridden field must be visible
        prop_assert!(
            visible.contains(granted_field),
            "Field {} was granted via override but not visible",
            granted_field
        );

        // Only the granted field should be visible
        prop_assert_eq!(
            visible.len(),
            1,
            "Only the override-granted field should be visible, got {:?}",
            visible
        );
    }
}

// ============================================================
// Determinism
// ============================================================

proptest! {
    /// Visibility resolution is deterministic: same inputs always yield same outputs.
    #[test]
    fn visibility_is_deterministic(
        num_fields in 1..8usize,
        shown_mask in prop::collection::vec(any::<bool>(), 1..8),
        contact_id in contact_id_strategy(),
    ) {
        let (mut card, field_ids) = card_with_fields(num_fields);

        for (i, fid) in field_ids.iter().enumerate() {
            if i < shown_mask.len() && shown_mask[i] {
                card.set_field_shown(fid, true);
            }
        }

        let label_manager = LabelManager::new();
        let result1 = resolve_visible_fields(&card, &label_manager, &contact_id);
        let result2 = resolve_visible_fields(&card, &label_manager, &contact_id);
        prop_assert_eq!(result1, result2, "Visibility resolution must be deterministic");
    }

    /// Determinism in groups mode with labels.
    #[test]
    fn visibility_deterministic_groups_mode(
        num_fields in 1..5usize,
        contact_id in contact_id_strategy(),
    ) {
        let (card, field_ids) = card_with_fields(num_fields);

        let mut label_manager = LabelManager::new();
        let label = label_manager.create_label("Group1").unwrap();
        let label_id = label.id().to_string();

        label_manager.add_contact_to_label(&label_id, &contact_id).unwrap();

        if !field_ids.is_empty() {
            let label_mut = label_manager.get_label_mut(&label_id).unwrap();
            label_mut.add_visible_field(&field_ids[0]);
        }

        let result1 = resolve_visible_fields(&card, &label_manager, &contact_id);
        let result2 = resolve_visible_fields(&card, &label_manager, &contact_id);
        prop_assert_eq!(result1, result2, "Groups mode visibility must be deterministic");
    }
}

// ============================================================
// Mode switching invariant
// ============================================================

proptest! {
    /// When switching from no-group to groups mode (by creating a label),
    /// contacts not in any label lose all visibility (default-closed).
    #[test]
    fn mode_switch_default_closed(
        num_fields in 1..8usize,
        shown_count in 1..8usize,
    ) {
        let (mut card, field_ids) = card_with_fields(num_fields);
        let shown_count = shown_count.min(num_fields);

        // Mark some fields as shown (no-group mode)
        for fid in field_ids.iter().take(shown_count) {
            card.set_field_shown(fid, true);
        }

        // Verify no-group mode works
        let empty_manager = LabelManager::new();
        let visible_no_groups = resolve_visible_fields(&card, &empty_manager, "contact-1");
        prop_assert!(
            !visible_no_groups.is_empty(),
            "Should have visible fields in no-group mode"
        );

        // Switch to groups mode by creating a label (without adding the contact)
        let mut label_manager = LabelManager::new();
        label_manager.create_label("SomeGroup").unwrap();

        let visible_groups = resolve_visible_fields(&card, &label_manager, "contact-1");
        prop_assert!(
            visible_groups.is_empty(),
            "Ungrouped contact should see nothing in groups mode, saw {:?}",
            visible_groups
        );
    }
}

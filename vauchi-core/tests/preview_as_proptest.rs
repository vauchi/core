// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-Based Tests: Preview-As Visibility Correctness
//!
//! Verifies correctness invariants of the visibility engine used by the
//! preview-as rendering path. `build_shared_info` calls
//! `get_effective_field_visibility` per field to decide Shown/Hidden.
//! These tests verify that function is consistent with the rules stored.
//!
//! Priority model (from `get_effective_field_visibility`):
//!   1. Per-contact override (if set) — highest priority
//!   2. Group membership (visible if contact is in any group showing the field)
//!   3. Contact's `VisibilityRules` fallback (default: Everyone → visible)
//!
//! Traces to: _private/features/visibility.feature @visibility @preview-as

mod common;

use proptest::prelude::*;

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

// ============================================================
// Helpers
// ============================================================

/// Add `field_count` email fields to the own card, return field IDs.
fn add_own_fields(vauchi: &Vauchi, field_count: usize) -> Vec<String> {
    let mut field_ids = Vec::new();
    for i in 0..field_count {
        let field = ContactField::new(
            FieldType::Email,
            &format!("field_{}", i),
            &format!("user{}@example.com", i),
        );
        let fid = field.id().to_string();
        vauchi.add_own_field(field).unwrap();
        field_ids.push(fid);
    }
    field_ids
}

/// Add a contact and return its ID.
fn add_contact(vauchi: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

// ============================================================
// Property: per-contact override takes priority over group visibility
// ============================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// A hide override must make a group-visible field invisible.
    /// Priority rule 1 must beat priority rule 2.
    #[test]
    fn override_hides_group_visible_field(
        field_count in 1..8usize,
        override_index in 0..8usize,
    ) {
        let override_index = override_index % field_count;

        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Dave");

        // Group with ALL fields visible, contact is a member
        let group = vauchi.create_group("AllVisible").unwrap();
        let gid = group.id().to_string();
        vauchi.add_contact_to_group(&gid, &contact_id).unwrap();
        for fid in &field_ids {
            vauchi.set_group_field_visibility(&gid, fid, true).unwrap();
        }

        // Override: explicitly hide one field from this contact
        let hidden_field = &field_ids[override_index];
        vauchi
            .set_contact_visibility_override(&contact_id, hidden_field, false)
            .unwrap();

        // The overridden field must be hidden despite the group grant
        let is_visible = vauchi
            .get_effective_field_visibility(&contact_id, hidden_field)
            .unwrap();
        prop_assert!(
            !is_visible,
            "Override (hide) must beat group grant for field '{}'. \
             field_count={}, override_index={}",
            hidden_field,
            field_count,
            override_index,
        );

        // All other fields must remain visible via the group
        for (i, fid) in field_ids.iter().enumerate() {
            if i != override_index {
                let visible = vauchi
                    .get_effective_field_visibility(&contact_id, fid)
                    .unwrap();
                prop_assert!(
                    visible,
                    "Non-overridden field '{}' (index {}) must remain visible via group. \
                     field_count={}, override_index={}",
                    fid,
                    i,
                    field_count,
                    override_index,
                );
            }
        }
    }

    /// A show override must make a group-hidden field visible.
    /// Priority rule 1 must grant visibility even without a group grant.
    #[test]
    fn override_shows_group_hidden_field(
        field_count in 2..8usize,
        grant_index in 0..8usize,
    ) {
        let grant_index = grant_index % field_count;

        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Eve");

        // Group with NO visible fields, contact is a member
        let group = vauchi.create_group("EmptyGroup").unwrap();
        let gid = group.id().to_string();
        vauchi.add_contact_to_group(&gid, &contact_id).unwrap();

        // Override: grant visibility to one specific field
        let granted_field = &field_ids[grant_index];
        vauchi
            .set_contact_visibility_override(&contact_id, granted_field, true)
            .unwrap();

        // The granted field must be visible
        let is_visible = vauchi
            .get_effective_field_visibility(&contact_id, granted_field)
            .unwrap();
        prop_assert!(
            is_visible,
            "Override (show) must grant visibility to field '{}'. \
             field_count={}, grant_index={}",
            granted_field,
            field_count,
            grant_index,
        );
    }

    // ============================================================
    // Property: group membership determines visibility (no overrides)
    // ============================================================

    /// A contact in a group sees exactly the fields that group makes visible
    /// (when no per-contact overrides exist and no fallback VisibilityRules are set).
    ///
    /// To test this cleanly we need to isolate from the VisibilityRules fallback.
    /// We do that by creating a group with explicit field grants — the union of
    /// those grants is what the contact should see.
    #[test]
    fn group_member_sees_group_visible_fields(
        field_count in 2..8usize,
        visible_mask in prop::collection::vec(any::<bool>(), 2..8),
    ) {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Frank");

        let group = vauchi.create_group("TestGroup").unwrap();
        let gid = group.id().to_string();
        vauchi.add_contact_to_group(&gid, &contact_id).unwrap();

        // Track which fields are actually granted via group
        let mut group_granted: Vec<bool> = vec![false; field_count];
        for (i, fid) in field_ids.iter().enumerate() {
            let show = i < visible_mask.len() && visible_mask[i];
            if show {
                vauchi.set_group_field_visibility(&gid, fid, true).unwrap();
                group_granted[i] = true;
            }
        }

        // Per-field visibility must match what we told the group
        // Fields not in any group with the contact fall back to VisibilityRules
        // (default: Everyone → true). We only assert consistency for granted fields.
        for (i, fid) in field_ids.iter().enumerate() {
            if group_granted[i] {
                let visible = vauchi
                    .get_effective_field_visibility(&contact_id, fid)
                    .unwrap();
                prop_assert!(
                    visible,
                    "Field '{}' was granted via group but get_effective returned false. \
                     field_count={}, index={}, visible_mask={:?}",
                    fid,
                    field_count,
                    i,
                    visible_mask,
                );
            }
        }
    }

    // ============================================================
    // Property: override removal restores group-based visibility
    // ============================================================

    /// After removing a hide-override, the field returns to its group-based
    /// visibility (visible if in the group's visible_fields, hidden otherwise).
    #[test]
    fn override_removal_restores_group_visibility(
        field_count in 1..6usize,
        field_index in 0..6usize,
    ) {
        let field_index = field_index % field_count;

        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Grace");

        // Group makes the chosen field visible
        let group = vauchi.create_group("MyGroup").unwrap();
        let gid = group.id().to_string();
        vauchi.add_contact_to_group(&gid, &contact_id).unwrap();
        let target_field = &field_ids[field_index];
        vauchi
            .set_group_field_visibility(&gid, target_field, true)
            .unwrap();

        // Add a hide-override → field becomes hidden
        vauchi
            .set_contact_visibility_override(&contact_id, target_field, false)
            .unwrap();
        let after_hide = vauchi
            .get_effective_field_visibility(&contact_id, target_field)
            .unwrap();
        prop_assert!(
            !after_hide,
            "Field '{}' must be hidden after setting hide-override. \
             field_count={}, field_index={}",
            target_field,
            field_count,
            field_index,
        );

        // Remove the override → field must revert to group-granted (visible)
        vauchi
            .remove_contact_visibility_override(&contact_id, target_field)
            .unwrap();
        let after_removal = vauchi
            .get_effective_field_visibility(&contact_id, target_field)
            .unwrap();
        prop_assert!(
            after_removal,
            "Field '{}' must revert to visible (group grant) after override removal. \
             field_count={}, field_index={}",
            target_field,
            field_count,
            field_index,
        );
    }

    // ============================================================
    // Property: multi-group union — contact sees union of all fields
    // ============================================================

    /// A contact in multiple groups sees the union of all fields visible
    /// across those groups. Each group contributes its visible_fields set,
    /// and a field visible in ANY group must be visible to the contact.
    #[test]
    fn multi_group_union_is_preserved(
        field_count in 3..8usize,
        num_groups in 2..4usize,
    ) {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Henry");

        let mut union_fields: Vec<String> = Vec::new();

        for g_idx in 0..num_groups {
            let group = vauchi.create_group(&format!("Group_{}", g_idx)).unwrap();
            let gid = group.id().to_string();
            vauchi.add_contact_to_group(&gid, &contact_id).unwrap();

            // Each group shows one distinct field (if enough fields)
            if g_idx < field_count {
                vauchi
                    .set_group_field_visibility(&gid, &field_ids[g_idx], true)
                    .unwrap();
                if !union_fields.contains(&field_ids[g_idx]) {
                    union_fields.push(field_ids[g_idx].clone());
                }
            }
        }

        // Every field in the union must be visible
        for fid in &union_fields {
            let visible = vauchi
                .get_effective_field_visibility(&contact_id, fid)
                .unwrap();
            prop_assert!(
                visible,
                "Field '{}' is in group union but get_effective returned false. \
                 field_count={}, num_groups={}",
                fid,
                field_count,
                num_groups,
            );
        }
    }

    // ============================================================
    // Property: determinism — same state always produces same result
    // ============================================================

    /// Calling `get_effective_field_visibility` twice on identical state
    /// must return the same result (no side effects, pure read).
    #[test]
    fn visibility_query_is_deterministic(
        field_count in 1..6usize,
        seed in any::<u32>(),
    ) {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Iris");

        // Set up some random visibility using seed
        let group = vauchi.create_group("DeterminismGroup").unwrap();
        let gid = group.id().to_string();
        if seed & 1 == 1 {
            vauchi.add_contact_to_group(&gid, &contact_id).unwrap();
        }
        for (i, fid) in field_ids.iter().enumerate() {
            let show_bit = (seed >> (i + 1)) & 1;
            if show_bit == 1 {
                vauchi.set_group_field_visibility(&gid, fid, true).unwrap();
            }
        }

        // Query each field twice — results must be identical
        for fid in &field_ids {
            let first = vauchi
                .get_effective_field_visibility(&contact_id, fid)
                .unwrap();
            let second = vauchi
                .get_effective_field_visibility(&contact_id, fid)
                .unwrap();
            prop_assert_eq!(
                first,
                second,
                "get_effective_field_visibility must be deterministic for field '{}'. \
                 field_count={}, seed={}",
                fid,
                field_count,
                seed,
            );
        }
    }

    // ============================================================
    // Property: build_shared_info iteration consistency
    // ============================================================

    /// `build_shared_info` iterates own-card fields and calls
    /// `get_effective_field_visibility` per field. This property verifies
    /// that calling it once per field vs. collecting all results in a loop
    /// produces consistent outcomes — a regression guard against any
    /// stateful side-effects in the visibility engine.
    #[test]
    fn bulk_iteration_agrees_with_individual_queries(
        field_count in 1..8usize,
        seed in any::<u64>(),
    ) {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("Owner").unwrap();
        let field_ids = add_own_fields(&vauchi, field_count);
        let contact_id = add_contact(&vauchi, "Jack");

        // Configure visibility via groups + overrides using seed bits
        let num_groups = ((seed & 0x3) as usize) + 1;
        let mut group_ids = Vec::new();
        for i in 0..num_groups {
            let g = vauchi.create_group(&format!("G{}", i)).unwrap();
            group_ids.push(g.id().to_string());
        }
        // Assign contact to groups based on seed
        for (g_idx, gid) in group_ids.iter().enumerate() {
            if (seed >> (4 + g_idx)) & 1 == 1 {
                vauchi.add_contact_to_group(gid, &contact_id).unwrap();
            }
        }
        // Assign fields to groups based on seed
        for (f_idx, fid) in field_ids.iter().enumerate() {
            for (g_idx, gid) in group_ids.iter().enumerate() {
                if (seed >> (8 + f_idx * 4 + g_idx)) & 1 == 1 {
                    vauchi.set_group_field_visibility(gid, fid, true).unwrap();
                }
            }
        }
        // Per-contact override for first field based on seed
        if field_count >= 1 && (seed >> 32) & 1 == 1 {
            let override_val = (seed >> 33) & 1 == 1;
            vauchi
                .set_contact_visibility_override(&contact_id, &field_ids[0], override_val)
                .unwrap();
        }

        // Collect all results in a batch (simulating build_shared_info loop)
        let batch: Vec<bool> = field_ids
            .iter()
            .map(|fid| {
                vauchi
                    .get_effective_field_visibility(&contact_id, fid)
                    .unwrap_or(true)
            })
            .collect();

        // Query each field individually and compare
        for (i, fid) in field_ids.iter().enumerate() {
            let individual = vauchi
                .get_effective_field_visibility(&contact_id, fid)
                .unwrap_or(true);
            prop_assert_eq!(
                individual,
                batch[i],
                "Field '{}' (index {}): individual query ({}) disagrees with \
                 batch result ({}). field_count={}, seed={}",
                fid,
                i,
                individual,
                batch[i],
                field_count,
                seed,
            );
        }
    }
}

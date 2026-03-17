// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extended Visibility Label Tests
//!
//! Covers @implemented scenarios not exercised by visibility_label_tests.rs:
//!   - Display name override per label (@display-name-override)
//!   - Label template application to new contacts (@template)
//!   - Per-contact visibility override (focused) (@override)
//!   - Edge cases: empty labels, rename preserves data, delete-contact cascade
//!   - Effective visibility view across labels + overrides
//!
//! Traces to: features/visibility_labels.feature

use std::collections::HashSet;

use vauchi_core::contact::{GroupError, GroupManager, MAX_LABELS, SUGGESTED_LABELS};

// =============================================================================
// Helpers
// =============================================================================

fn create_manager_with_labels(names: &[&str]) -> (GroupManager, Vec<String>) {
    let mut manager = GroupManager::new();
    let mut ids = Vec::new();
    for name in names {
        let label = manager.create_group(name).unwrap();
        ids.push(label.id().to_string());
    }
    (manager, ids)
}

// =============================================================================
// @display-name-override: Per-group display name override
// =============================================================================
// Scenario: Per-group display name override
//   Given I have a group "Business"
//   When I set the display name override to "Dr. Egloff"
//   Then contacts in "Business" should see my name as "Dr. Egloff"
//   When I clear the display name override
//   Then contacts in "Business" should see my default name

/// Tests display name override lifecycle: set, resolve, clear, re-resolve.
// @scenario: visibility_labels.feature:Per-group display name override
#[test]
fn test_display_name_override_lifecycle() {
    let mut manager = GroupManager::new();

    let biz = manager.create_group("Business").unwrap();
    let biz_id = biz.id().to_string();

    // Initially no override — default name is returned.
    let label = manager.get_group(&biz_id).unwrap();
    assert_eq!(label.display_name_override(), None);
    assert_eq!(label.resolve_display_name("Alice Smith"), "Alice Smith");

    // Set override.
    let label = manager.get_group_mut(&biz_id).unwrap();
    label
        .set_display_name_override(Some("Dr. Egloff"))
        .expect("valid override");
    assert_eq!(label.display_name_override(), Some("Dr. Egloff"));
    assert_eq!(label.resolve_display_name("Alice Smith"), "Dr. Egloff");

    // Clear override — falls back to default.
    let label = manager.get_group_mut(&biz_id).unwrap();
    label
        .set_display_name_override(None)
        .expect("clearing should work");
    assert_eq!(label.display_name_override(), None);
    assert_eq!(label.resolve_display_name("Alice Smith"), "Alice Smith");
}

/// Tests display name override validation (empty, whitespace, too-long).
// @scenario: visibility_labels.feature:Per-group display name override (validation)
#[test]
fn test_display_name_override_validation() {
    let mut manager = GroupManager::new();
    let label_id = manager.create_group("Test").unwrap().id().to_string();

    let label = manager.get_group_mut(&label_id).unwrap();

    // Empty string → error.
    let err = label.set_display_name_override(Some("")).unwrap_err();
    assert!(matches!(err, GroupError::InvalidName(_)));

    // Whitespace-only → error.
    let err = label.set_display_name_override(Some("   ")).unwrap_err();
    assert!(matches!(err, GroupError::InvalidName(_)));

    // > 100 chars → error.
    let long = "x".repeat(101);
    let err = label.set_display_name_override(Some(&long)).unwrap_err();
    assert!(matches!(err, GroupError::InvalidName(_)));

    // Exactly 100 chars → success.
    let max = "a".repeat(100);
    label
        .set_display_name_override(Some(&max))
        .expect("100 chars should succeed");
    assert_eq!(label.display_name_override().map(|s| s.len()), Some(100));

    // Trimming of surrounding whitespace.
    label
        .set_display_name_override(Some("  Trimmed  "))
        .expect("trimmed name should succeed");
    assert_eq!(label.display_name_override(), Some("Trimmed"));
}

/// Tests that different labels can have independent display name overrides.
// @scenario: visibility_labels.feature:Per-group display name override (independent per label)
#[test]
fn test_display_name_override_independent_per_label() {
    let (mut manager, ids) = create_manager_with_labels(&["Business", "Family", "Casual"]);

    // Set different overrides on two labels; leave third without.
    manager
        .get_group_mut(&ids[0])
        .unwrap()
        .set_display_name_override(Some("Dr. Egloff"))
        .unwrap();
    manager
        .get_group_mut(&ids[1])
        .unwrap()
        .set_display_name_override(Some("Matt"))
        .unwrap();

    let biz = manager.get_group(&ids[0]).unwrap();
    let fam = manager.get_group(&ids[1]).unwrap();
    let cas = manager.get_group(&ids[2]).unwrap();

    assert_eq!(biz.resolve_display_name("Default"), "Dr. Egloff");
    assert_eq!(fam.resolve_display_name("Default"), "Matt");
    assert_eq!(cas.resolve_display_name("Default"), "Default");
}

// =============================================================================
// @template: Apply label template to new contact
// =============================================================================
// Scenario: Configure default fields for a label
// Scenario: Apply label template to new contact

/// Tests that a label's configured fields act as a template when adding new contacts.
/// When a contact is added, they immediately see the fields configured for the label.
// @scenario: visibility_labels.feature:Apply label template to new contact
#[test]
fn test_template_applies_to_new_contact() {
    let mut manager = GroupManager::new();
    let pro = manager.create_group("Professional").unwrap();
    let pro_id = pro.id().to_string();

    // Configure the label with work-only fields (template).
    let label = manager.get_group_mut(&pro_id).unwrap();
    let work_fields: HashSet<String> = ["work-phone".to_string(), "work-email".to_string()]
        .into_iter()
        .collect();
    label.set_visible_fields(work_fields);

    // Verify personal fields are NOT visible.
    assert!(!label.is_field_visible("personal-phone"));
    assert!(!label.is_field_visible("personal-email"));

    // Add Eve to the label.
    manager.add_contact_to_group(&pro_id, "eve-id").unwrap();

    // Eve should immediately see exactly the template fields.
    let eve_visible = manager.visible_fields_via_labels("eve-id");
    assert_eq!(eve_visible.len(), 2);
    assert!(eve_visible.contains("work-phone"));
    assert!(eve_visible.contains("work-email"));
    assert!(!eve_visible.contains("personal-phone"));
    assert!(!eve_visible.contains("personal-email"));
}

/// Tests that per-contact override takes precedence even with a template.
// @scenario: visibility_labels.feature:Apply label template to new contact (with override)
#[test]
fn test_template_overridden_by_per_contact() {
    let mut manager = GroupManager::new();
    let pro_id = manager
        .create_group("Professional")
        .unwrap()
        .id()
        .to_string();

    // Configure template fields.
    let label = manager.get_group_mut(&pro_id).unwrap();
    label.add_visible_field("work-phone");
    label.add_visible_field("work-email");

    // Eve has a pre-existing override hiding work-phone.
    manager.set_contact_override("eve-id", "work-phone", false);

    // Add Eve to the label.
    manager.add_contact_to_group(&pro_id, "eve-id").unwrap();

    // Eve should see work-email but NOT work-phone (override wins).
    let eve_visible = manager.visible_fields_via_labels("eve-id");
    assert!(eve_visible.contains("work-email"));
    assert!(
        !eve_visible.contains("work-phone"),
        "per-contact override should hide work-phone despite template"
    );
}

// =============================================================================
// @override: Focused per-contact override scenarios
// =============================================================================

/// Tests granting visibility to a contact NOT in any label.
// @scenario: visibility_labels.feature:Grant visibility to contact not in label
#[test]
fn test_override_grant_to_non_member() {
    let mut manager = GroupManager::new();
    let fam_id = manager.create_group("Family").unwrap().id().to_string();

    // Configure Family with home-address visible.
    let label = manager.get_group_mut(&fam_id).unwrap();
    label.add_visible_field("home-address");

    // Dave is NOT in Family.
    assert_eq!(
        manager.can_see_via_labels("dave-id", "home-address"),
        None,
        "dave should not see home-address via labels"
    );

    // Grant Dave home-address via per-contact override.
    manager.set_contact_override("dave-id", "home-address", true);
    assert_eq!(
        manager.can_see_via_labels("dave-id", "home-address"),
        Some(true),
        "dave should see home-address via override"
    );

    // Dave does NOT need to be in Family.
    assert!(manager.groups_for_contact("dave-id").is_empty());
}

/// Tests revoking visibility from a contact who IS in a label.
// @scenario: visibility_labels.feature:Revoke visibility from contact in label
#[test]
fn test_override_revoke_from_member() {
    let mut manager = GroupManager::new();
    let friends_id = manager.create_group("Friends").unwrap().id().to_string();

    let label = manager.get_group_mut(&friends_id).unwrap();
    label.add_visible_field("personal-phone");

    // Carol is in Friends.
    manager
        .add_contact_to_group(&friends_id, "carol-id")
        .unwrap();
    assert_eq!(
        manager.can_see_via_labels("carol-id", "personal-phone"),
        Some(true)
    );

    // Specifically hide personal-phone from Carol.
    manager.set_contact_override("carol-id", "personal-phone", false);
    assert_eq!(
        manager.can_see_via_labels("carol-id", "personal-phone"),
        Some(false),
        "Carol should NOT see personal-phone despite being in Friends"
    );
}

/// Tests viewing effective visibility across labels + overrides.
// @scenario: visibility_labels.feature:View effective visibility for a contact
#[test]
fn test_effective_visibility_view() {
    let (mut manager, ids) = create_manager_with_labels(&["Family", "Friends"]);
    let fam_id = &ids[0];
    let fri_id = &ids[1];

    // Family shows: home-address, personal-phone.
    let label = manager.get_group_mut(fam_id).unwrap();
    label.add_visible_field("home-address");
    label.add_visible_field("personal-phone");

    // Friends shows: personal-phone, personal-email.
    let label = manager.get_group_mut(fri_id).unwrap();
    label.add_visible_field("personal-phone");
    label.add_visible_field("personal-email");

    // Carol is in both.
    manager.add_contact_to_group(fam_id, "carol-id").unwrap();
    manager.add_contact_to_group(fri_id, "carol-id").unwrap();

    // Override: hide home-address from Carol.
    manager.set_contact_override("carol-id", "home-address", false);
    // Override: grant work-email to Carol (not in any label).
    manager.set_contact_override("carol-id", "work-email", true);

    let visible = manager.visible_fields_via_labels("carol-id");
    // Should have: personal-phone (Family+Friends), personal-email (Friends), work-email (override grant).
    // Should NOT have: home-address (override hide).
    assert!(visible.contains("personal-phone"));
    assert!(visible.contains("personal-email"));
    assert!(visible.contains("work-email"));
    assert!(!visible.contains("home-address"));
    assert_eq!(visible.len(), 3);

    // Check individual overrides are inspectable.
    assert_eq!(
        manager.get_contact_override("carol-id", "home-address"),
        Some(false)
    );
    assert_eq!(
        manager.get_contact_override("carol-id", "work-email"),
        Some(true)
    );
    let all = manager.get_all_contact_overrides("carol-id").unwrap();
    assert_eq!(all.len(), 2);
}

/// Tests clearing per-contact overrides restores label-only visibility.
// @scenario: visibility_labels.feature:Clear per-contact overrides
#[test]
fn test_clear_per_contact_overrides() {
    let mut manager = GroupManager::new();
    let fam_id = manager.create_group("Family").unwrap().id().to_string();

    let label = manager.get_group_mut(&fam_id).unwrap();
    label.add_visible_field("home-address");
    label.add_visible_field("personal-phone");

    manager.add_contact_to_group(&fam_id, "dave-id").unwrap();

    // Set overrides for Dave.
    manager.set_contact_override("dave-id", "home-address", false);
    manager.set_contact_override("dave-id", "secret-field", true);

    // Before clearing: home-address hidden, secret-field granted.
    let visible = manager.visible_fields_via_labels("dave-id");
    assert!(!visible.contains("home-address"));
    assert!(visible.contains("secret-field"));

    // Clear all overrides.
    manager.clear_contact_overrides("dave-id");

    // After clearing: label-only visibility.
    assert!(manager.get_all_contact_overrides("dave-id").is_none());
    let visible = manager.visible_fields_via_labels("dave-id");
    assert!(
        visible.contains("home-address"),
        "label visibility restored"
    );
    assert!(visible.contains("personal-phone"));
    assert!(
        !visible.contains("secret-field"),
        "override-granted field should be gone"
    );
}

// =============================================================================
// @edge-cases
// =============================================================================

/// Tests that a label with no contacts still exists and is configurable.
// @scenario: visibility_labels.feature:Label with no contacts still exists
#[test]
fn test_empty_label_persists() {
    let mut manager = GroupManager::new();
    let label = manager.create_group("Future Team").unwrap();
    let label_id = label.id().to_string();

    // Verify it exists with zero contacts.
    assert_eq!(label.contact_count(), 0);
    assert!(label.contacts().is_empty());

    // Should appear in all_groups.
    assert_eq!(manager.group_count(), 1);
    let all = manager.all_groups();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name(), "Future Team");

    // Should be configurable: add fields.
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_visible_field("work-email");
    label.add_visible_field("work-phone");

    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 2);
    assert!(label.is_field_visible("work-email"));
}

/// Tests that renaming a label preserves its contacts and field associations.
// @scenario: visibility_labels.feature:Rename an existing label (data preservation)
#[test]
fn test_rename_preserves_contacts_and_fields() {
    let mut manager = GroupManager::new();
    let label = manager.create_group("Work").unwrap();
    let label_id = label.id().to_string();

    // Add contacts and fields.
    manager.add_contact_to_group(&label_id, "bob-id").unwrap();
    manager.add_contact_to_group(&label_id, "carol-id").unwrap();
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_visible_field("work-email");
    label.add_visible_field("work-phone");
    label
        .set_display_name_override(Some("Professional Me"))
        .unwrap();

    // Rename.
    manager.rename_group(&label_id, "Colleagues").unwrap();

    // All data preserved.
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.name(), "Colleagues");
    assert_eq!(label.contact_count(), 2);
    assert!(label.contains_contact("bob-id"));
    assert!(label.contains_contact("carol-id"));
    assert!(label.is_field_visible("work-email"));
    assert!(label.is_field_visible("work-phone"));
    assert_eq!(label.display_name_override(), Some("Professional Me"));
}

/// Tests that renaming to an existing label name fails.
// @scenario: visibility_labels.feature:Cannot rename to existing label name
#[test]
fn test_rename_to_existing_name_fails() {
    let (mut manager, ids) = create_manager_with_labels(&["Friends", "Family"]);

    let err = manager.rename_group(&ids[0], "Family").unwrap_err();
    assert!(matches!(err, GroupError::DuplicateName(_)));

    // Label should remain unchanged.
    let label = manager.get_group(&ids[0]).unwrap();
    assert_eq!(label.name(), "Friends");
}

/// Tests that deleting a contact removes them from all labels and clears overrides.
// @scenario: visibility_labels.feature:Delete contact removes from all labels
#[test]
fn test_delete_contact_removes_from_all_labels_and_overrides() {
    let (mut manager, ids) = create_manager_with_labels(&["Friends", "Colleagues"]);

    let dave = "dave-id";

    // Dave is in both labels.
    manager.add_contact_to_group(&ids[0], dave).unwrap();
    manager.add_contact_to_group(&ids[1], dave).unwrap();
    manager.set_contact_override(dave, "phone", true);
    manager.set_contact_override(dave, "email", false);

    assert_eq!(manager.groups_for_contact(dave).len(), 2);
    assert_eq!(manager.get_all_contact_overrides(dave).unwrap().len(), 2);

    // Simulate contact deletion.
    manager.remove_contact_from_all_groups(dave);

    // Dave should be gone from all labels.
    assert_eq!(manager.groups_for_contact(dave).len(), 0);
    for id in &ids {
        assert!(!manager.get_group(id).unwrap().contains_contact(dave));
    }

    // All overrides cleared.
    assert!(manager.get_all_contact_overrides(dave).is_none());
}

/// Tests maximum labels boundary: create 50, fail on 51st, delete + re-create.
// @scenario: visibility_labels.feature:Maximum number of labels
#[test]
fn test_max_labels_boundary() {
    let mut manager = GroupManager::new();

    // Fill to max.
    for i in 0..MAX_LABELS {
        manager.create_group(&format!("Label-{:03}", i)).unwrap();
    }
    assert_eq!(manager.group_count(), MAX_LABELS);

    // 51st label fails.
    let err = manager.create_group("Overflow").unwrap_err();
    assert!(matches!(err, GroupError::MaxLabelsReached));

    // Delete one, then create succeeds.
    let first_id = manager.all_groups().first().unwrap().id().to_string();
    manager.delete_group(&first_id).unwrap();
    assert_eq!(manager.group_count(), MAX_LABELS - 1);

    manager
        .create_group("Replacement")
        .expect("should succeed after deletion");
    assert_eq!(manager.group_count(), MAX_LABELS);
}

/// Tests that creating a label with a duplicate name is case-sensitive.
// @scenario: visibility_labels.feature:Cannot create duplicate label names (case-sensitivity)
#[test]
fn test_duplicate_name_is_case_sensitive() {
    let mut manager = GroupManager::new();
    manager.create_group("Friends").unwrap();

    // "friends" (lowercase) is different — should succeed.
    manager
        .create_group("friends")
        .expect("lowercase variant should be distinct");
    assert_eq!(manager.group_count(), 2);

    // Exact match fails.
    let err = manager.create_group("Friends").unwrap_err();
    assert!(matches!(err, GroupError::DuplicateName(_)));
}

/// Tests name trimming: leading/trailing whitespace is stripped.
// @scenario: visibility_labels.feature:Create custom label with any name (trimming)
#[test]
fn test_label_name_trimming() {
    let mut manager = GroupManager::new();

    let label = manager.create_group("  University Colleagues  ").unwrap();
    assert_eq!(label.name(), "University Colleagues");

    // Duplicate check uses trimmed name.
    let err = manager.create_group("University Colleagues").unwrap_err();
    assert!(matches!(err, GroupError::DuplicateName(_)));
}

/// Tests suggested labels constant matches the feature spec.
// @scenario: visibility_labels.feature:Default labels are suggested on first use
#[test]
fn test_suggested_labels() {
    assert_eq!(SUGGESTED_LABELS.len(), 4);
    assert!(SUGGESTED_LABELS.contains(&"Family"));
    assert!(SUGGESTED_LABELS.contains(&"Friends"));
    // Can be created with one tap — just create_group for each.
    let mut manager = GroupManager::new();
    for name in SUGGESTED_LABELS {
        manager
            .create_group(name)
            .expect("suggested label should be creatable");
    }
    assert_eq!(manager.group_count(), SUGGESTED_LABELS.len());
}

// =============================================================================
// @local-only: Labels are local and never exposed to contacts
// =============================================================================

/// Tests that label names are never part of contact-visible data structures.
/// The GroupManager resolves visibility to a set of field IDs — no label names leak.
// @scenario: visibility_labels.feature:Labels are not shared with contacts
// @scenario: visibility_labels.feature:Labels exist only on my devices
#[test]
fn test_labels_are_local_only() {
    let mut manager = GroupManager::new();
    let fam_id = manager
        .create_group("Annoying People")
        .unwrap()
        .id()
        .to_string();

    let label = manager.get_group_mut(&fam_id).unwrap();
    label.add_visible_field("work-email");
    manager.add_contact_to_group(&fam_id, "bob-id").unwrap();

    // The only thing Bob's side can observe is which fields are visible,
    // NOT the label name. visible_fields_via_labels returns field IDs only.
    let bob_visible = manager.visible_fields_via_labels("bob-id");
    assert!(bob_visible.contains("work-email"));

    // The label name "Annoying People" is never in the visible-fields set.
    assert!(!bob_visible.contains("Annoying People"));

    // can_see_via_labels also returns bool, not label metadata.
    assert_eq!(
        manager.can_see_via_labels("bob-id", "work-email"),
        Some(true)
    );
}

// =============================================================================
// @field-label: Remove field from label visibility
// =============================================================================

/// Tests removing a field from a label's visibility list.
// @scenario: visibility_labels.feature:Remove field from label visibility
#[test]
fn test_remove_field_from_label_visibility() {
    let mut manager = GroupManager::new();
    let fam_id = manager.create_group("Family").unwrap().id().to_string();

    let label = manager.get_group_mut(&fam_id).unwrap();
    label.add_visible_field("personal-email");
    label.add_visible_field("home-address");

    manager.add_contact_to_group(&fam_id, "bob-id").unwrap();

    // Bob can see both fields.
    assert_eq!(
        manager.can_see_via_labels("bob-id", "personal-email"),
        Some(true)
    );

    // Remove personal-email from Family.
    let label = manager.get_group_mut(&fam_id).unwrap();
    let removed = label.remove_visible_field("personal-email");
    assert!(removed, "field should have been present");

    // Bob can no longer see personal-email via labels.
    assert_eq!(manager.can_see_via_labels("bob-id", "personal-email"), None);
    // But home-address is still visible.
    assert_eq!(
        manager.can_see_via_labels("bob-id", "home-address"),
        Some(true)
    );

    // Unless Bob has a per-contact override for personal-email.
    manager.set_contact_override("bob-id", "personal-email", true);
    assert_eq!(
        manager.can_see_via_labels("bob-id", "personal-email"),
        Some(true),
        "override should restore visibility"
    );
}

/// Tests associating a field with multiple labels.
// @scenario: visibility_labels.feature:Associate field with multiple labels
#[test]
fn test_field_visible_in_multiple_labels() {
    let (mut manager, ids) = create_manager_with_labels(&["Family", "Close Friends"]);

    // Set home-address visible in both labels.
    for id in &ids {
        let label = manager.get_group_mut(id).unwrap();
        label.add_visible_field("home-address");
    }

    // Bob in Family, Carol in Close Friends, Dave in neither.
    manager.add_contact_to_group(&ids[0], "bob-id").unwrap();
    manager.add_contact_to_group(&ids[1], "carol-id").unwrap();

    // Both should see home-address.
    assert_eq!(
        manager.can_see_via_labels("bob-id", "home-address"),
        Some(true)
    );
    assert_eq!(
        manager.can_see_via_labels("carol-id", "home-address"),
        Some(true)
    );
    // Dave should not.
    assert_eq!(manager.can_see_via_labels("dave-id", "home-address"), None);
}

/// Tests viewing which labels can see a specific field (with contact counts).
// @scenario: visibility_labels.feature:View which labels can see a field
#[test]
fn test_view_labels_showing_field() {
    let (mut manager, ids) = create_manager_with_labels(&["Family", "Close Friends", "Work"]);

    // Family and Close Friends show personal-phone; Work does not.
    manager
        .get_group_mut(&ids[0])
        .unwrap()
        .add_visible_field("personal-phone");
    manager
        .get_group_mut(&ids[1])
        .unwrap()
        .add_visible_field("personal-phone");
    manager
        .get_group_mut(&ids[2])
        .unwrap()
        .add_visible_field("work-phone");

    // Add some contacts for count verification.
    manager.add_contact_to_group(&ids[0], "bob").unwrap();
    manager.add_contact_to_group(&ids[0], "carol").unwrap();
    manager.add_contact_to_group(&ids[1], "dave").unwrap();

    let all_labels = manager.all_groups();
    let showing_phone: Vec<(&str, usize)> = all_labels
        .iter()
        .filter(|l| l.is_field_visible("personal-phone"))
        .map(|l| (l.name(), l.contact_count()))
        .collect();

    assert_eq!(showing_phone.len(), 2);
    let names: HashSet<&str> = showing_phone.iter().map(|(n, _)| *n).collect();
    assert!(names.contains("Family"));
    assert!(names.contains("Close Friends"));

    // Verify contact counts are available.
    let family_entry = showing_phone.iter().find(|(n, _)| *n == "Family").unwrap();
    assert_eq!(family_entry.1, 2);
    let friends_entry = showing_phone
        .iter()
        .find(|(n, _)| *n == "Close Friends")
        .unwrap();
    assert_eq!(friends_entry.1, 1);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Label Tests
//!
//! Comprehensive tests for visibility labels functionality including CRUD operations,
//! contact assignment, cascading visibility changes, field visibility, and device sync.
//!
//! Traces to: features/visibility_labels.feature
//! - @label-create, @label-rename, @label-delete: Label CRUD operations
//! - @assign-contact: Contact assignment to labels
//! - @visibility-effect: Cascading visibility changes
//! - @field-label: Label-based field visibility
//! - @local-only: Labels sync across devices

use std::collections::HashSet;

use vauchi_core::contact::{GroupError, GroupManager, MAX_LABELS};

// =============================================================================
// Test Helpers
// =============================================================================

fn create_manager_with_labels(names: &[&str]) -> (GroupManager, Vec<String>) {
    let mut manager = GroupManager::new();
    let mut label_ids = Vec::new();

    for name in names {
        let label = manager.create_group(name).unwrap();
        label_ids.push(label.id().to_string());
    }

    (manager, label_ids)
}

// =============================================================================
// Test 1: Label CRUD Operations
// =============================================================================
// Traces to: visibility_labels.feature
// - @label-create: Create a new visibility label
// - @label-create: Create custom label with any name
// - @label-create: Cannot create duplicate label names
// - @label-rename: Rename an existing label
// - @label-rename: Cannot rename to existing label name
// - @label-delete: Delete a label

/// Tests the complete lifecycle of label CRUD operations: Create, Read, Update, Delete.
///
/// Feature: visibility_labels.feature
/// Scenarios:
/// - Create a new visibility label
/// - Default labels are suggested on first use
/// - Create custom label with any name
/// - Cannot create duplicate label names
/// - Rename an existing label
/// - Cannot rename to existing label name
/// - Delete a label
// @scenario: visibility_labels :: Create a new visibility label
// @scenario: visibility_labels :: Cannot create duplicate label names
// @scenario: visibility_labels :: Rename an existing label
// @scenario: visibility_labels :: Delete a label
// @scenario: contacts_management :: Delete a group
// @scenario: contacts_management :: Rename a group
#[test]
fn test_label_crud_operations() {
    let mut manager = GroupManager::new();

    // =========================================================================
    // CREATE - Basic label creation
    // =========================================================================

    // Create a new label
    let family = manager.create_group("Family").unwrap();
    assert_eq!(family.name(), "Family");
    assert_eq!(family.contact_count(), 0);
    assert!(family.visible_fields().is_empty());
    let family_id = family.id().to_string();

    // Verify it exists
    assert_eq!(manager.group_count(), 1);
    manager.get_group(&family_id).expect("expected Some");
    manager.get_group_by_name("Family").expect("expected Some");

    // Create additional labels
    let friends = manager.create_group("Friends").unwrap();
    let friends_id = friends.id().to_string();

    let professional = manager.create_group("Professional").unwrap();
    let professional_id = professional.id().to_string();

    assert_eq!(manager.group_count(), 3);

    // =========================================================================
    // CREATE - Error cases
    // =========================================================================

    // Cannot create duplicate label name
    let duplicate_result = manager.create_group("Family");
    assert!(matches!(
        duplicate_result,
        Err(GroupError::DuplicateName(_))
    ));

    // Cannot create label with empty name
    let empty_result = manager.create_group("");
    assert!(matches!(empty_result, Err(GroupError::InvalidName(_))));

    // Cannot create label with only whitespace
    let whitespace_result = manager.create_group("   ");
    assert!(matches!(whitespace_result, Err(GroupError::InvalidName(_))));

    // Cannot create label with name exceeding 50 characters
    let long_name = "A".repeat(51);
    let long_result = manager.create_group(&long_name);
    assert!(matches!(long_result, Err(GroupError::InvalidName(_))));

    // =========================================================================
    // READ - Retrieve labels
    // =========================================================================

    // Get by ID
    let retrieved = manager.get_group(&family_id).unwrap();
    assert_eq!(retrieved.name(), "Family");

    // Get by name
    let by_name = manager.get_group_by_name("Friends").unwrap();
    assert_eq!(by_name.id(), friends_id);

    // List all labels
    let all_labels = manager.all_groups();
    assert_eq!(all_labels.len(), 3);

    // =========================================================================
    // UPDATE - Rename labels
    // =========================================================================

    // Rename a label
    manager.rename_group(&friends_id, "Close Friends").unwrap();
    let renamed = manager.get_group(&friends_id).unwrap();
    assert_eq!(renamed.name(), "Close Friends");

    // Old name should no longer work
    assert!(manager.get_group_by_name("Friends").is_none());

    // New name should work
    manager
        .get_group_by_name("Close Friends")
        .expect("expected Some");

    // Cannot rename to existing label name
    let rename_dup_result = manager.rename_group(&friends_id, "Family");
    assert!(matches!(
        rename_dup_result,
        Err(GroupError::DuplicateName(_))
    ));

    // Cannot rename to empty name
    let rename_empty_result = manager.rename_group(&friends_id, "");
    assert!(matches!(
        rename_empty_result,
        Err(GroupError::InvalidName(_))
    ));

    // Cannot rename non-existent label
    let rename_missing_result = manager.rename_group("non-existent-id", "New Name");
    assert!(matches!(
        rename_missing_result,
        Err(GroupError::NotFound(_))
    ));

    // Renaming to same name is allowed (no-op)
    manager.rename_group(&family_id, "Family").unwrap();
    assert_eq!(manager.get_group(&family_id).unwrap().name(), "Family");

    // =========================================================================
    // DELETE - Remove labels
    // =========================================================================

    // Add a contact to a label before deletion
    manager
        .add_contact_to_group(&professional_id, "bob-id")
        .unwrap();

    // Delete the label
    let deleted = manager.delete_group(&professional_id).unwrap();
    assert_eq!(deleted.name(), "Professional");

    // Label should no longer exist
    assert!(manager.get_group(&professional_id).is_none());
    assert!(manager.get_group_by_name("Professional").is_none());
    assert_eq!(manager.group_count(), 2);

    // Cannot delete non-existent label
    let delete_missing_result = manager.delete_group("non-existent-id");
    assert!(matches!(
        delete_missing_result,
        Err(GroupError::NotFound(_))
    ));

    // =========================================================================
    // Verify suggested labels constant
    // =========================================================================
    use vauchi_core::contact::SUGGESTED_LABELS;
    assert_eq!(
        SUGGESTED_LABELS,
        &["Family", "Friends", "Coworkers", "Business"]
    );
}

/// Tests that the maximum number of labels is enforced.
///
/// Feature: visibility_labels.feature
/// Scenario: Maximum number of labels
// @scenario: visibility_labels :: Maximum number of labels
#[test]
fn test_label_max_limit() {
    let mut manager = GroupManager::new();

    // Create maximum number of labels
    for i in 0..MAX_LABELS {
        manager.create_group(&format!("Label{}", i)).unwrap();
    }

    assert_eq!(manager.group_count(), MAX_LABELS);

    // Cannot create one more
    let result = manager.create_group("OneMore");
    assert!(matches!(result, Err(GroupError::MaxLabelsReached)));

    // Delete one label
    let label_ids: Vec<String> = manager
        .all_groups()
        .iter()
        .map(|l| l.id().to_string())
        .collect();
    manager.delete_group(&label_ids[0]).unwrap();

    // Now we can create one more
    manager.create_group("NewLabel").unwrap();
    assert_eq!(manager.group_count(), MAX_LABELS);
}

// =============================================================================
// Test 2: Contact Assignment to Labels
// =============================================================================
// Traces to: visibility_labels.feature
// - @assign-contact: Add a contact to a label
// - @assign-contact: Add multiple contacts to a label at once
// - @assign-contact: Remove a contact from a label
// - @assign-contact: Contact in multiple labels
// - @assign-contact: View all labels for a contact

/// Tests adding and removing contacts from labels.
///
/// Feature: visibility_labels.feature
/// Scenarios:
/// - Add a contact to a label
/// - Add multiple contacts to a label at once
/// - Remove a contact from a label
/// - Contact in multiple labels
/// - View all labels for a contact
// @scenario: visibility_labels :: Add a contact to a label
// @scenario: visibility_labels :: Remove a contact from a label
// @scenario: visibility_labels :: Contact in multiple labels
// @scenario: contacts_management :: Contact in multiple groups
// @scenario: contacts_management :: Remove contact from group
// @scenario: contacts_management :: Filter contacts by group
#[test]
fn test_contact_assignment_to_label() {
    let (mut manager, label_ids) = create_manager_with_labels(&["Family", "Friends", "Colleagues"]);
    let family_id = &label_ids[0];
    let friends_id = &label_ids[1];
    let colleagues_id = &label_ids[2];

    // Contact IDs
    let bob = "bob-id";
    let carol = "carol-id";
    let dave = "dave-id";
    let eve = "eve-id";

    // =========================================================================
    // Add single contact to a label
    // =========================================================================

    // Add Bob to Family
    let added = manager.add_contact_to_group(family_id, bob).unwrap();
    assert!(added, "Bob should be newly added");

    // Verify Bob is in Family
    let family = manager.get_group(family_id).unwrap();
    assert!(family.contains_contact(bob));
    assert_eq!(family.contact_count(), 1);

    // Adding again returns false (already present)
    let added_again = manager.add_contact_to_group(family_id, bob).unwrap();
    assert!(!added_again, "Bob is already in Family");

    // =========================================================================
    // Add multiple contacts to a label
    // =========================================================================

    // Add Bob, Carol, and Dave to Friends
    manager.add_contact_to_group(friends_id, bob).unwrap();
    manager.add_contact_to_group(friends_id, carol).unwrap();
    manager.add_contact_to_group(friends_id, dave).unwrap();

    let friends = manager.get_group(friends_id).unwrap();
    assert_eq!(friends.contact_count(), 3);
    assert!(friends.contains_contact(bob));
    assert!(friends.contains_contact(carol));
    assert!(friends.contains_contact(dave));

    // =========================================================================
    // Contact in multiple labels
    // =========================================================================

    // Carol is also a colleague
    manager.add_contact_to_group(colleagues_id, carol).unwrap();

    // Carol should be in both Friends and Colleagues
    let carol_labels = manager.groups_for_contact(carol);
    assert_eq!(carol_labels.len(), 2);

    let carol_label_names: HashSet<&str> = carol_labels.iter().map(|l| l.name()).collect();
    assert!(carol_label_names.contains("Friends"));
    assert!(carol_label_names.contains("Colleagues"));

    // Bob is in Family and Friends
    let bob_labels = manager.groups_for_contact(bob);
    assert_eq!(bob_labels.len(), 2);

    // =========================================================================
    // Remove contact from label
    // =========================================================================

    // Remove Dave from Friends
    let removed = manager.remove_contact_from_group(friends_id, dave).unwrap();
    assert!(removed, "Dave should be removed");

    let friends = manager.get_group(friends_id).unwrap();
    assert!(!friends.contains_contact(dave));
    assert_eq!(friends.contact_count(), 2);

    // Removing again returns false (not present)
    let removed_again = manager.remove_contact_from_group(friends_id, dave).unwrap();
    assert!(!removed_again, "Dave is already not in Friends");

    // =========================================================================
    // Unlabeled contacts
    // =========================================================================

    // Eve is not in any label
    let all_contacts = vec![bob, carol, dave, eve];
    let unlabeled = manager.ungrouped_contacts(&all_contacts);

    assert_eq!(unlabeled.len(), 2);
    assert!(unlabeled.contains(&dave.to_string()));
    assert!(unlabeled.contains(&eve.to_string()));

    // =========================================================================
    // Remove contact from all labels (e.g., when deleting contact)
    // =========================================================================

    // Bob is in Family and Friends
    assert_eq!(manager.groups_for_contact(bob).len(), 2);

    // Remove Bob from all labels
    manager.remove_contact_from_all_groups(bob);

    // Bob should be in no labels
    assert_eq!(manager.groups_for_contact(bob).len(), 0);

    // =========================================================================
    // Error cases
    // =========================================================================

    // Cannot add to non-existent label
    let add_missing_result = manager.add_contact_to_group("non-existent", bob);
    assert!(matches!(add_missing_result, Err(GroupError::NotFound(_))));

    // Cannot remove from non-existent label
    let remove_missing_result = manager.remove_contact_from_group("non-existent", bob);
    assert!(matches!(
        remove_missing_result,
        Err(GroupError::NotFound(_))
    ));
}

// =============================================================================
// Test 3: Cascading Visibility Changes
// =============================================================================
// Traces to: visibility_labels.feature
// - @visibility-effect: Adding contact to label grants visibility
// - @visibility-effect: Removing contact from label revokes visibility
// - @visibility-effect: Contact sees fields based on label membership

/// Tests that visibility changes cascade properly when label membership changes.
///
/// Feature: visibility_labels.feature
/// Scenarios:
/// - Adding contact to label grants visibility
/// - Removing contact from label revokes visibility
/// - Contact sees fields based on label membership
// @scenario: visibility_labels :: Adding contact to label grants visibility
// @scenario: visibility_labels :: Removing contact from label revokes visibility
#[test]
fn test_cascading_visibility_changes() {
    let mut manager = GroupManager::new();

    // Create labels with different field visibility
    let family = manager.create_group("Family").unwrap();
    let family_id = family.id().to_string();

    let friends = manager.create_group("Friends").unwrap();
    let friends_id = friends.id().to_string();

    // Setup field visibility for Family: home-address, personal-phone
    let family = manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");
    family.add_visible_field("birthday");

    // Setup field visibility for Friends: personal-phone, personal-email
    let friends = manager.get_group_mut(&friends_id).unwrap();
    friends.add_visible_field("personal-phone");
    friends.add_visible_field("personal-email");

    // Contact IDs
    let bob = "bob-id";
    let carol = "carol-id";
    let dave = "dave-id";

    // =========================================================================
    // Initial state: No one sees any fields via labels
    // =========================================================================

    assert_eq!(manager.can_see_via_labels(bob, "home-address"), None);
    assert_eq!(manager.can_see_via_labels(bob, "personal-phone"), None);
    assert!(manager.visible_fields_via_labels(bob).is_empty());

    // =========================================================================
    // Adding Bob to Family grants visibility to Family fields
    // =========================================================================

    manager.add_contact_to_group(&family_id, bob).unwrap();

    // Bob should now see Family fields
    assert_eq!(
        manager.can_see_via_labels(bob, "home-address"),
        Some(true),
        "Bob should see home-address after joining Family"
    );
    assert_eq!(
        manager.can_see_via_labels(bob, "personal-phone"),
        Some(true),
        "Bob should see personal-phone after joining Family"
    );
    assert_eq!(
        manager.can_see_via_labels(bob, "birthday"),
        Some(true),
        "Bob should see birthday after joining Family"
    );

    // Bob should NOT see Friends-only fields
    assert_eq!(
        manager.can_see_via_labels(bob, "personal-email"),
        None,
        "Bob should not see personal-email (not in Friends)"
    );

    // Verify visible fields set
    let bob_visible = manager.visible_fields_via_labels(bob);
    assert_eq!(bob_visible.len(), 3);
    assert!(bob_visible.contains("home-address"));
    assert!(bob_visible.contains("personal-phone"));
    assert!(bob_visible.contains("birthday"));

    // =========================================================================
    // Adding Carol to both labels grants union of visibility
    // =========================================================================

    manager.add_contact_to_group(&family_id, carol).unwrap();
    manager.add_contact_to_group(&friends_id, carol).unwrap();

    // Carol should see all fields from both labels
    let carol_visible = manager.visible_fields_via_labels(carol);
    assert_eq!(carol_visible.len(), 4); // home-address, personal-phone, birthday, personal-email

    assert!(carol_visible.contains("home-address"));
    assert!(carol_visible.contains("personal-phone"));
    assert!(carol_visible.contains("birthday"));
    assert!(carol_visible.contains("personal-email"));

    // =========================================================================
    // Dave is only in Friends
    // =========================================================================

    manager.add_contact_to_group(&friends_id, dave).unwrap();

    let dave_visible = manager.visible_fields_via_labels(dave);
    assert_eq!(dave_visible.len(), 2); // personal-phone, personal-email
    assert!(!dave_visible.contains("home-address"));
    assert!(!dave_visible.contains("birthday"));

    // =========================================================================
    // Removing Bob from Family revokes visibility
    // =========================================================================

    manager.remove_contact_from_group(&family_id, bob).unwrap();

    // Bob should no longer see Family fields
    assert_eq!(
        manager.can_see_via_labels(bob, "home-address"),
        None,
        "Bob should not see home-address after leaving Family"
    );
    assert_eq!(
        manager.can_see_via_labels(bob, "birthday"),
        None,
        "Bob should not see birthday after leaving Family"
    );

    // Bob is now in no labels
    assert!(manager.visible_fields_via_labels(bob).is_empty());

    // =========================================================================
    // Removing Carol from Friends reduces her visibility
    // =========================================================================

    manager
        .remove_contact_from_group(&friends_id, carol)
        .unwrap();

    // Carol should still see Family fields but not Friends-only fields
    let carol_visible = manager.visible_fields_via_labels(carol);
    assert_eq!(carol_visible.len(), 3); // Only Family fields now
    assert!(carol_visible.contains("home-address"));
    assert!(carol_visible.contains("personal-phone"));
    assert!(carol_visible.contains("birthday"));
    assert!(!carol_visible.contains("personal-email")); // Friends-only

    // =========================================================================
    // Adding field to label cascades to all contacts in label
    // =========================================================================

    let family = manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("emergency-contact");

    // Carol (still in Family) should now see this field
    let carol_visible = manager.visible_fields_via_labels(carol);
    assert!(carol_visible.contains("emergency-contact"));

    // =========================================================================
    // Removing field from label cascades to all contacts
    // =========================================================================

    let family = manager.get_group_mut(&family_id).unwrap();
    family.remove_visible_field("home-address");

    // Carol should no longer see home-address
    let carol_visible = manager.visible_fields_via_labels(carol);
    assert!(!carol_visible.contains("home-address"));
}

// =============================================================================
// Test 4: Label-Based Field Visibility
// =============================================================================
// Traces to: visibility_labels.feature
// - @field-label: Associate a field with a label
// - @field-label: Associate field with multiple labels
// - @field-label: Remove field from label visibility
// - @field-label: View which labels can see a field
// - @override: Per-contact override takes precedence over label

/// Tests label-based field visibility with per-contact overrides.
///
/// Feature: visibility_labels.feature
/// Scenarios:
/// - Associate a field with a label
/// - Associate field with multiple labels
/// - Remove field from label visibility
/// - Per-contact override takes precedence over label
// @scenario: visibility_labels :: Associate a field with a label
// @scenario: visibility_labels :: Per-contact override takes precedence over label
#[test]
fn test_label_based_field_visibility() {
    let mut manager = GroupManager::new();

    // Create labels
    let family = manager.create_group("Family").unwrap();
    let family_id = family.id().to_string();

    let close_friends = manager.create_group("Close Friends").unwrap();
    let close_friends_id = close_friends.id().to_string();

    let colleagues = manager.create_group("Colleagues").unwrap();
    let colleagues_id = colleagues.id().to_string();

    // Contact IDs
    let bob = "bob-id";
    let carol = "carol-id";
    let dave = "dave-id";
    let eve = "eve-id";

    // =========================================================================
    // Associate fields with labels
    // =========================================================================

    // Family sees: home-address, personal-phone
    let family = manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    // Close Friends sees: personal-phone, personal-email
    let close_friends = manager.get_group_mut(&close_friends_id).unwrap();
    close_friends.add_visible_field("personal-phone");
    close_friends.add_visible_field("personal-email");

    // Colleagues sees: work-email, work-phone
    let colleagues = manager.get_group_mut(&colleagues_id).unwrap();
    colleagues.add_visible_field("work-email");
    colleagues.add_visible_field("work-phone");

    // =========================================================================
    // Assign contacts to labels
    // =========================================================================

    // Bob is Family
    manager.add_contact_to_group(&family_id, bob).unwrap();

    // Carol is Close Friend and Colleague
    manager
        .add_contact_to_group(&close_friends_id, carol)
        .unwrap();
    manager.add_contact_to_group(&colleagues_id, carol).unwrap();

    // Dave is Colleague only
    manager.add_contact_to_group(&colleagues_id, dave).unwrap();

    // Eve is not in any label

    // =========================================================================
    // Test field visibility per contact
    // =========================================================================

    // Bob sees Family fields
    assert_eq!(manager.can_see_via_labels(bob, "home-address"), Some(true));
    assert_eq!(
        manager.can_see_via_labels(bob, "personal-phone"),
        Some(true)
    );
    assert_eq!(manager.can_see_via_labels(bob, "personal-email"), None);
    assert_eq!(manager.can_see_via_labels(bob, "work-email"), None);

    // Carol sees Close Friends + Colleagues (union)
    assert_eq!(
        manager.can_see_via_labels(carol, "personal-phone"),
        Some(true)
    );
    assert_eq!(
        manager.can_see_via_labels(carol, "personal-email"),
        Some(true)
    );
    assert_eq!(manager.can_see_via_labels(carol, "work-email"), Some(true));
    assert_eq!(manager.can_see_via_labels(carol, "work-phone"), Some(true));
    assert_eq!(manager.can_see_via_labels(carol, "home-address"), None); // Family only

    // Dave sees only Colleagues fields
    assert_eq!(manager.can_see_via_labels(dave, "work-email"), Some(true));
    assert_eq!(manager.can_see_via_labels(dave, "work-phone"), Some(true));
    assert_eq!(manager.can_see_via_labels(dave, "personal-email"), None);

    // Eve (not in any label) sees nothing via labels
    assert_eq!(manager.can_see_via_labels(eve, "work-email"), None);
    assert_eq!(manager.can_see_via_labels(eve, "home-address"), None);

    // =========================================================================
    // Per-contact override: Grant visibility to non-member
    // =========================================================================

    // Eve is not in Family, but we grant her home-address specifically
    manager.set_contact_override(eve, "home-address", true);

    assert_eq!(
        manager.can_see_via_labels(eve, "home-address"),
        Some(true),
        "Eve should see home-address via per-contact override"
    );

    // =========================================================================
    // Per-contact override: Revoke visibility from member
    // =========================================================================

    // Carol is in Close Friends which shows personal-email
    // But we specifically hide it from her
    manager.set_contact_override(carol, "personal-email", false);

    assert_eq!(
        manager.can_see_via_labels(carol, "personal-email"),
        Some(false),
        "Carol should NOT see personal-email due to per-contact override"
    );

    // Carol still sees other fields from her labels
    assert_eq!(
        manager.can_see_via_labels(carol, "personal-phone"),
        Some(true)
    );
    assert_eq!(manager.can_see_via_labels(carol, "work-email"), Some(true));

    // =========================================================================
    // Override takes precedence over label
    // =========================================================================

    // Bob is in Family which shows home-address
    // Override to hide it
    manager.set_contact_override(bob, "home-address", false);
    assert_eq!(
        manager.can_see_via_labels(bob, "home-address"),
        Some(false),
        "Per-contact override should hide field despite label membership"
    );

    // Bob's visible_fields_via_labels should reflect the override
    let bob_visible = manager.visible_fields_via_labels(bob);
    assert!(!bob_visible.contains("home-address"));
    assert!(bob_visible.contains("personal-phone")); // Still visible

    // =========================================================================
    // Remove override restores label visibility
    // =========================================================================

    manager.remove_contact_override(bob, "home-address");

    assert_eq!(
        manager.can_see_via_labels(bob, "home-address"),
        Some(true),
        "After removing override, label visibility should apply"
    );

    // =========================================================================
    // Clear all overrides for a contact
    // =========================================================================

    // Carol has a hidden personal-email override
    assert_eq!(
        manager.get_contact_override(carol, "personal-email"),
        Some(false)
    );

    manager.clear_contact_overrides(carol);

    // Override should be gone, label visibility applies
    assert_eq!(manager.get_contact_override(carol, "personal-email"), None);
    assert_eq!(
        manager.can_see_via_labels(carol, "personal-email"),
        Some(true)
    );

    // =========================================================================
    // View which labels show a field
    // =========================================================================

    // personal-phone is in Family and Close Friends
    let labels = manager.all_groups();
    let labels_showing_phone: Vec<&str> = labels
        .iter()
        .filter(|l| l.is_field_visible("personal-phone"))
        .map(|l| l.name())
        .collect();

    assert_eq!(labels_showing_phone.len(), 2);
    assert!(labels_showing_phone.contains(&"Family"));
    assert!(labels_showing_phone.contains(&"Close Friends"));

    // work-email is only in Colleagues
    let labels_showing_work_email: Vec<&str> = labels
        .iter()
        .filter(|l| l.is_field_visible("work-email"))
        .map(|l| l.name())
        .collect();

    assert_eq!(labels_showing_work_email.len(), 1);
    assert!(labels_showing_work_email.contains(&"Colleagues"));
}

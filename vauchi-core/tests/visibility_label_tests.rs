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

use vauchi_core::contact::{LabelError, LabelManager, VisibilityLabel, MAX_LABELS};
use vauchi_core::sync::SyncItem;

// =============================================================================
// Test Helpers
// =============================================================================

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn create_manager_with_labels(names: &[&str]) -> (LabelManager, Vec<String>) {
    let mut manager = LabelManager::new();
    let mut label_ids = Vec::new();

    for name in names {
        let label = manager.create_label(name).unwrap();
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
#[test]
fn test_label_crud_operations() {
    let mut manager = LabelManager::new();

    // =========================================================================
    // CREATE - Basic label creation
    // =========================================================================

    // Create a new label
    let family = manager.create_label("Family").unwrap();
    assert_eq!(family.name(), "Family");
    assert_eq!(family.contact_count(), 0);
    assert!(family.visible_fields().is_empty());
    let family_id = family.id().to_string();

    // Verify it exists
    assert_eq!(manager.label_count(), 1);
    assert!(manager.get_label(&family_id).is_some());
    assert!(manager.get_label_by_name("Family").is_some());

    // Create additional labels
    let friends = manager.create_label("Friends").unwrap();
    let friends_id = friends.id().to_string();

    let professional = manager.create_label("Professional").unwrap();
    let professional_id = professional.id().to_string();

    assert_eq!(manager.label_count(), 3);

    // =========================================================================
    // CREATE - Error cases
    // =========================================================================

    // Cannot create duplicate label name
    let duplicate_result = manager.create_label("Family");
    assert!(matches!(
        duplicate_result,
        Err(LabelError::DuplicateName(_))
    ));

    // Cannot create label with empty name
    let empty_result = manager.create_label("");
    assert!(matches!(empty_result, Err(LabelError::InvalidName(_))));

    // Cannot create label with only whitespace
    let whitespace_result = manager.create_label("   ");
    assert!(matches!(whitespace_result, Err(LabelError::InvalidName(_))));

    // Cannot create label with name exceeding 50 characters
    let long_name = "A".repeat(51);
    let long_result = manager.create_label(&long_name);
    assert!(matches!(long_result, Err(LabelError::InvalidName(_))));

    // =========================================================================
    // READ - Retrieve labels
    // =========================================================================

    // Get by ID
    let retrieved = manager.get_label(&family_id).unwrap();
    assert_eq!(retrieved.name(), "Family");

    // Get by name
    let by_name = manager.get_label_by_name("Friends").unwrap();
    assert_eq!(by_name.id(), friends_id);

    // List all labels
    let all_labels = manager.all_labels();
    assert_eq!(all_labels.len(), 3);

    // =========================================================================
    // UPDATE - Rename labels
    // =========================================================================

    // Rename a label
    manager.rename_label(&friends_id, "Close Friends").unwrap();
    let renamed = manager.get_label(&friends_id).unwrap();
    assert_eq!(renamed.name(), "Close Friends");

    // Old name should no longer work
    assert!(manager.get_label_by_name("Friends").is_none());

    // New name should work
    assert!(manager.get_label_by_name("Close Friends").is_some());

    // Cannot rename to existing label name
    let rename_dup_result = manager.rename_label(&friends_id, "Family");
    assert!(matches!(
        rename_dup_result,
        Err(LabelError::DuplicateName(_))
    ));

    // Cannot rename to empty name
    let rename_empty_result = manager.rename_label(&friends_id, "");
    assert!(matches!(
        rename_empty_result,
        Err(LabelError::InvalidName(_))
    ));

    // Cannot rename non-existent label
    let rename_missing_result = manager.rename_label("non-existent-id", "New Name");
    assert!(matches!(
        rename_missing_result,
        Err(LabelError::NotFound(_))
    ));

    // Renaming to same name is allowed (no-op)
    manager.rename_label(&family_id, "Family").unwrap();
    assert_eq!(manager.get_label(&family_id).unwrap().name(), "Family");

    // =========================================================================
    // DELETE - Remove labels
    // =========================================================================

    // Add a contact to a label before deletion
    manager
        .add_contact_to_label(&professional_id, "bob-id")
        .unwrap();

    // Delete the label
    let deleted = manager.delete_label(&professional_id).unwrap();
    assert_eq!(deleted.name(), "Professional");

    // Label should no longer exist
    assert!(manager.get_label(&professional_id).is_none());
    assert!(manager.get_label_by_name("Professional").is_none());
    assert_eq!(manager.label_count(), 2);

    // Cannot delete non-existent label
    let delete_missing_result = manager.delete_label("non-existent-id");
    assert!(matches!(
        delete_missing_result,
        Err(LabelError::NotFound(_))
    ));

    // =========================================================================
    // Verify suggested labels constant
    // =========================================================================
    use vauchi_core::contact::SUGGESTED_LABELS;
    assert_eq!(SUGGESTED_LABELS, &["Family", "Friends", "Professional"]);
}

/// Tests that the maximum number of labels is enforced.
///
/// Feature: visibility_labels.feature
/// Scenario: Maximum number of labels
#[test]
fn test_label_max_limit() {
    let mut manager = LabelManager::new();

    // Create maximum number of labels
    for i in 0..MAX_LABELS {
        manager.create_label(&format!("Label{}", i)).unwrap();
    }

    assert_eq!(manager.label_count(), MAX_LABELS);

    // Cannot create one more
    let result = manager.create_label("OneMore");
    assert!(matches!(result, Err(LabelError::MaxLabelsReached)));

    // Delete one label
    let label_ids: Vec<String> = manager
        .all_labels()
        .iter()
        .map(|l| l.id().to_string())
        .collect();
    manager.delete_label(&label_ids[0]).unwrap();

    // Now we can create one more
    manager.create_label("NewLabel").unwrap();
    assert_eq!(manager.label_count(), MAX_LABELS);
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
    let added = manager.add_contact_to_label(family_id, bob).unwrap();
    assert!(added, "Bob should be newly added");

    // Verify Bob is in Family
    let family = manager.get_label(family_id).unwrap();
    assert!(family.contains_contact(bob));
    assert_eq!(family.contact_count(), 1);

    // Adding again returns false (already present)
    let added_again = manager.add_contact_to_label(family_id, bob).unwrap();
    assert!(!added_again, "Bob is already in Family");

    // =========================================================================
    // Add multiple contacts to a label
    // =========================================================================

    // Add Bob, Carol, and Dave to Friends
    manager.add_contact_to_label(friends_id, bob).unwrap();
    manager.add_contact_to_label(friends_id, carol).unwrap();
    manager.add_contact_to_label(friends_id, dave).unwrap();

    let friends = manager.get_label(friends_id).unwrap();
    assert_eq!(friends.contact_count(), 3);
    assert!(friends.contains_contact(bob));
    assert!(friends.contains_contact(carol));
    assert!(friends.contains_contact(dave));

    // =========================================================================
    // Contact in multiple labels
    // =========================================================================

    // Carol is also a colleague
    manager.add_contact_to_label(colleagues_id, carol).unwrap();

    // Carol should be in both Friends and Colleagues
    let carol_labels = manager.labels_for_contact(carol);
    assert_eq!(carol_labels.len(), 2);

    let carol_label_names: HashSet<&str> = carol_labels.iter().map(|l| l.name()).collect();
    assert!(carol_label_names.contains("Friends"));
    assert!(carol_label_names.contains("Colleagues"));

    // Bob is in Family and Friends
    let bob_labels = manager.labels_for_contact(bob);
    assert_eq!(bob_labels.len(), 2);

    // =========================================================================
    // Remove contact from label
    // =========================================================================

    // Remove Dave from Friends
    let removed = manager.remove_contact_from_label(friends_id, dave).unwrap();
    assert!(removed, "Dave should be removed");

    let friends = manager.get_label(friends_id).unwrap();
    assert!(!friends.contains_contact(dave));
    assert_eq!(friends.contact_count(), 2);

    // Removing again returns false (not present)
    let removed_again = manager.remove_contact_from_label(friends_id, dave).unwrap();
    assert!(!removed_again, "Dave is already not in Friends");

    // =========================================================================
    // Unlabeled contacts
    // =========================================================================

    // Eve is not in any label
    let all_contacts = vec![bob, carol, dave, eve];
    let unlabeled = manager.unlabeled_contacts(&all_contacts);

    assert_eq!(unlabeled.len(), 2);
    assert!(unlabeled.contains(&dave.to_string()));
    assert!(unlabeled.contains(&eve.to_string()));

    // =========================================================================
    // Remove contact from all labels (e.g., when deleting contact)
    // =========================================================================

    // Bob is in Family and Friends
    assert_eq!(manager.labels_for_contact(bob).len(), 2);

    // Remove Bob from all labels
    manager.remove_contact_from_all_labels(bob);

    // Bob should be in no labels
    assert_eq!(manager.labels_for_contact(bob).len(), 0);

    // =========================================================================
    // Error cases
    // =========================================================================

    // Cannot add to non-existent label
    let add_missing_result = manager.add_contact_to_label("non-existent", bob);
    assert!(matches!(add_missing_result, Err(LabelError::NotFound(_))));

    // Cannot remove from non-existent label
    let remove_missing_result = manager.remove_contact_from_label("non-existent", bob);
    assert!(matches!(
        remove_missing_result,
        Err(LabelError::NotFound(_))
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
#[test]
fn test_cascading_visibility_changes() {
    let mut manager = LabelManager::new();

    // Create labels with different field visibility
    let family = manager.create_label("Family").unwrap();
    let family_id = family.id().to_string();

    let friends = manager.create_label("Friends").unwrap();
    let friends_id = friends.id().to_string();

    // Setup field visibility for Family: home-address, personal-phone
    let family = manager.get_label_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");
    family.add_visible_field("birthday");

    // Setup field visibility for Friends: personal-phone, personal-email
    let friends = manager.get_label_mut(&friends_id).unwrap();
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

    manager.add_contact_to_label(&family_id, bob).unwrap();

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

    manager.add_contact_to_label(&family_id, carol).unwrap();
    manager.add_contact_to_label(&friends_id, carol).unwrap();

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

    manager.add_contact_to_label(&friends_id, dave).unwrap();

    let dave_visible = manager.visible_fields_via_labels(dave);
    assert_eq!(dave_visible.len(), 2); // personal-phone, personal-email
    assert!(!dave_visible.contains("home-address"));
    assert!(!dave_visible.contains("birthday"));

    // =========================================================================
    // Removing Bob from Family revokes visibility
    // =========================================================================

    manager.remove_contact_from_label(&family_id, bob).unwrap();

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
        .remove_contact_from_label(&friends_id, carol)
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

    let family = manager.get_label_mut(&family_id).unwrap();
    family.add_visible_field("emergency-contact");

    // Carol (still in Family) should now see this field
    let carol_visible = manager.visible_fields_via_labels(carol);
    assert!(carol_visible.contains("emergency-contact"));

    // =========================================================================
    // Removing field from label cascades to all contacts
    // =========================================================================

    let family = manager.get_label_mut(&family_id).unwrap();
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
#[test]
fn test_label_based_field_visibility() {
    let mut manager = LabelManager::new();

    // Create labels
    let family = manager.create_label("Family").unwrap();
    let family_id = family.id().to_string();

    let close_friends = manager.create_label("Close Friends").unwrap();
    let close_friends_id = close_friends.id().to_string();

    let colleagues = manager.create_label("Colleagues").unwrap();
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
    let family = manager.get_label_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    // Close Friends sees: personal-phone, personal-email
    let close_friends = manager.get_label_mut(&close_friends_id).unwrap();
    close_friends.add_visible_field("personal-phone");
    close_friends.add_visible_field("personal-email");

    // Colleagues sees: work-email, work-phone
    let colleagues = manager.get_label_mut(&colleagues_id).unwrap();
    colleagues.add_visible_field("work-email");
    colleagues.add_visible_field("work-phone");

    // =========================================================================
    // Assign contacts to labels
    // =========================================================================

    // Bob is Family
    manager.add_contact_to_label(&family_id, bob).unwrap();

    // Carol is Close Friend and Colleague
    manager
        .add_contact_to_label(&close_friends_id, carol)
        .unwrap();
    manager.add_contact_to_label(&colleagues_id, carol).unwrap();

    // Dave is Colleague only
    manager.add_contact_to_label(&colleagues_id, dave).unwrap();

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
    let labels = manager.all_labels();
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

// =============================================================================
// Test 5: Label Sync Across Devices
// =============================================================================
// Traces to: visibility_labels.feature
// - @local-only: Labels sync across my own devices only
// - @local-only: Labels are not shared with contacts

/// Tests that labels sync across the user's own devices via SyncItem::LabelChange.
///
/// Feature: visibility_labels.feature
/// Scenarios:
/// - Labels sync across my own devices only
/// - Labels are not shared with contacts
#[test]
fn test_label_sync_across_devices() {
    // =========================================================================
    // Setup: Device A creates labels
    // =========================================================================

    let mut device_a_manager = LabelManager::new();

    let family = device_a_manager.create_label("Family").unwrap();
    let family_id = family.id().to_string();

    let friends = device_a_manager.create_label("Close Friends").unwrap();
    let friends_id = friends.id().to_string();

    // Add contacts and fields
    device_a_manager
        .add_contact_to_label(&family_id, "bob-id")
        .unwrap();
    device_a_manager
        .add_contact_to_label(&family_id, "carol-id")
        .unwrap();
    device_a_manager
        .add_contact_to_label(&friends_id, "dave-id")
        .unwrap();

    let family = device_a_manager.get_label_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    let friends = device_a_manager.get_label_mut(&friends_id).unwrap();
    friends.add_visible_field("personal-email");

    // =========================================================================
    // Create SyncItem::LabelChange for syncing to Device B
    // =========================================================================

    let timestamp = now();

    // Family label sync item
    let family_label = device_a_manager.get_label(&family_id).unwrap();
    let family_sync = SyncItem::LabelChange {
        label_id: family_label.id().to_string(),
        label_name: family_label.name().to_string(),
        contacts: family_label.contacts().iter().cloned().collect(),
        visible_fields: family_label.visible_fields().iter().cloned().collect(),
        is_deleted: false,
        timestamp,
    };

    // Friends label sync item (used for deletion test below)
    let friends_label = device_a_manager.get_label(&friends_id).unwrap();
    let _friends_sync = SyncItem::LabelChange {
        label_id: friends_label.id().to_string(),
        label_name: friends_label.name().to_string(),
        contacts: friends_label.contacts().iter().cloned().collect(),
        visible_fields: friends_label.visible_fields().iter().cloned().collect(),
        is_deleted: false,
        timestamp,
    };

    // =========================================================================
    // Verify SyncItem contents
    // =========================================================================

    // Verify Family sync item
    if let SyncItem::LabelChange {
        label_id,
        label_name,
        contacts,
        visible_fields,
        is_deleted,
        timestamp: ts,
    } = &family_sync
    {
        assert_eq!(label_id, &family_id);
        assert_eq!(label_name, "Family");
        assert_eq!(contacts.len(), 2);
        assert!(contacts.contains(&"bob-id".to_string()));
        assert!(contacts.contains(&"carol-id".to_string()));
        assert_eq!(visible_fields.len(), 2);
        assert!(visible_fields.contains(&"home-address".to_string()));
        assert!(visible_fields.contains(&"personal-phone".to_string()));
        assert!(!is_deleted);
        assert_eq!(*ts, timestamp);
    } else {
        panic!("Expected LabelChange variant");
    }

    // =========================================================================
    // Simulate Device B receiving the sync and reconstructing labels
    // =========================================================================

    // Apply Family sync - simulate Device B receiving and applying it
    if let SyncItem::LabelChange {
        label_id,
        label_name,
        contacts,
        visible_fields,
        is_deleted: false,
        ..
    } = family_sync
    {
        // Create label from sync data (as Device B would)
        let label = VisibilityLabel::from_storage(
            label_id,
            label_name,
            contacts.into_iter().collect(),
            visible_fields.into_iter().collect(),
            timestamp,
            timestamp,
        );

        // Verify the label can be reconstructed correctly on Device B
        assert_eq!(label.name(), "Family");
        assert_eq!(label.contact_count(), 2);
        assert!(label.contains_contact("bob-id"));
        assert!(label.contains_contact("carol-id"));
        assert!(label.is_field_visible("home-address"));
        assert!(label.is_field_visible("personal-phone"));
    }

    // =========================================================================
    // Test label deletion sync
    // =========================================================================

    let deleted_sync = SyncItem::LabelChange {
        label_id: friends_id.clone(),
        label_name: "Close Friends".to_string(),
        contacts: vec![],
        visible_fields: vec![],
        is_deleted: true,
        timestamp: timestamp + 1,
    };

    if let SyncItem::LabelChange {
        label_id,
        is_deleted,
        ..
    } = &deleted_sync
    {
        assert_eq!(label_id, &friends_id);
        assert!(is_deleted, "is_deleted should be true for deletion");
    }

    // =========================================================================
    // Verify SyncItem timestamp accessor
    // =========================================================================

    let sync_item = SyncItem::LabelChange {
        label_id: "test-id".to_string(),
        label_name: "Test".to_string(),
        contacts: vec![],
        visible_fields: vec![],
        is_deleted: false,
        timestamp: 12345,
    };

    assert_eq!(sync_item.timestamp(), 12345);

    // =========================================================================
    // Test conflict resolution for labels
    // =========================================================================

    // Device A updates label at timestamp 1000
    let item_a = SyncItem::LabelChange {
        label_id: family_id.clone(),
        label_name: "Family Members".to_string(),
        contacts: vec!["bob-id".to_string()],
        visible_fields: vec!["home-address".to_string()],
        is_deleted: false,
        timestamp: 1000,
    };

    // Device B updates same label at timestamp 2000 (later)
    let item_b = SyncItem::LabelChange {
        label_id: family_id.clone(),
        label_name: "Close Family".to_string(),
        contacts: vec!["bob-id".to_string(), "carol-id".to_string()],
        visible_fields: vec!["home-address".to_string(), "birthday".to_string()],
        is_deleted: false,
        timestamp: 2000,
    };

    let device_a_id = [0xAA; 32];
    let device_b_id = [0xBB; 32];

    // Later timestamp wins
    let resolved = SyncItem::resolve_conflict(&item_a, &item_b, &device_a_id, &device_b_id);

    if let SyncItem::LabelChange {
        label_name,
        contacts,
        visible_fields,
        timestamp: ts,
        ..
    } = resolved
    {
        assert_eq!(label_name, "Close Family");
        assert_eq!(contacts.len(), 2);
        assert_eq!(visible_fields.len(), 2);
        assert_eq!(ts, 2000);
    } else {
        panic!("Expected LabelChange variant");
    }

    // =========================================================================
    // Verify labels are local-only (not transmitted to contacts)
    // =========================================================================

    // This is a design verification - labels exist in LabelManager, not in Contact
    // The contact card sent to others never contains label information
    // Only field visibility is transmitted (what they can see), not why (labels)

    let label = device_a_manager.get_label(&family_id).unwrap();

    // The label stores contact IDs internally
    assert!(label.contains_contact("bob-id"));

    // But Bob receives field visibility, not label membership
    // This is enforced by the fact that Contact and ContactCard have no label fields
    // The visibility is computed via can_see_via_labels() and applied when sending updates

    // Label names are never serialized in contact updates
    // Only the resulting visibility decision is shared
}

/// Tests that per-contact overrides are cleared when contact is removed from all labels.
///
/// Feature: visibility_labels.feature
/// Scenario: Delete contact removes from all labels
#[test]
fn test_delete_contact_clears_overrides() {
    let mut manager = LabelManager::new();

    // Create labels
    let family_id = manager.create_label("Family").unwrap().id().to_string();
    let friends_id = manager.create_label("Friends").unwrap().id().to_string();

    let bob = "bob-id";

    // Add Bob to both labels
    manager.add_contact_to_label(&family_id, bob).unwrap();
    manager.add_contact_to_label(&friends_id, bob).unwrap();

    // Set some per-contact overrides
    manager.set_contact_override(bob, "field-1", true);
    manager.set_contact_override(bob, "field-2", false);

    // Verify state
    assert_eq!(manager.labels_for_contact(bob).len(), 2);
    assert!(manager.get_all_contact_overrides(bob).is_some());

    // Remove Bob from all labels (simulates contact deletion)
    manager.remove_contact_from_all_labels(bob);

    // Bob should be in no labels
    assert_eq!(manager.labels_for_contact(bob).len(), 0);

    // Per-contact overrides should be cleared
    assert!(manager.get_all_contact_overrides(bob).is_none());
    assert_eq!(manager.get_contact_override(bob, "field-1"), None);
    assert_eq!(manager.get_contact_override(bob, "field-2"), None);
}

/// Tests set_visible_fields bulk operation on a label.
///
/// Feature: visibility_labels.feature
/// Scenario: Configure default fields for a label
#[test]
fn test_set_visible_fields_bulk() {
    let mut manager = LabelManager::new();

    let label_id = manager
        .create_label("Professional")
        .unwrap()
        .id()
        .to_string();

    // Set multiple fields at once
    let work_fields: HashSet<String> = [
        "work-email".to_string(),
        "work-phone".to_string(),
        "company".to_string(),
        "title".to_string(),
    ]
    .into_iter()
    .collect();

    let label = manager.get_label_mut(&label_id).unwrap();
    label.set_visible_fields(work_fields);

    // Verify all fields are set
    let label = manager.get_label(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 4);
    assert!(label.is_field_visible("work-email"));
    assert!(label.is_field_visible("work-phone"));
    assert!(label.is_field_visible("company"));
    assert!(label.is_field_visible("title"));
    assert!(!label.is_field_visible("personal-email"));

    // Replace with different set
    let minimal_fields: HashSet<String> = ["work-email".to_string()].into_iter().collect();

    let label = manager.get_label_mut(&label_id).unwrap();
    label.set_visible_fields(minimal_fields);

    // Verify replacement
    let label = manager.get_label(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 1);
    assert!(label.is_field_visible("work-email"));
    assert!(!label.is_field_visible("work-phone"));
}

/// Tests label timestamp updates (created_at, modified_at).
///
/// Feature: visibility_labels.feature
/// Scenario: Labels track creation and modification times
#[test]
fn test_label_timestamps() {
    let mut manager = LabelManager::new();

    let label = manager.create_label("Test Label").unwrap();
    let label_id = label.id().to_string();

    let created_at = label.created_at();
    let initial_modified = label.modified_at();

    // Initially, created_at and modified_at should be equal
    assert_eq!(created_at, initial_modified);

    // Wait a tiny bit and modify the label
    std::thread::sleep(std::time::Duration::from_millis(10));

    let label = manager.get_label_mut(&label_id).unwrap();
    label.set_name("Updated Label");

    let label = manager.get_label(&label_id).unwrap();

    // created_at should be unchanged
    assert_eq!(label.created_at(), created_at);

    // modified_at should be updated (or equal if within same second)
    assert!(label.modified_at() >= initial_modified);

    // Modify again by adding a field
    std::thread::sleep(std::time::Duration::from_millis(10));

    let label = manager.get_label_mut(&label_id).unwrap();
    label.add_visible_field("test-field");

    let label = manager.get_label(&label_id).unwrap();
    assert!(label.modified_at() >= initial_modified);

    // Modify by adding a contact
    let prev_modified = label.modified_at();
    std::thread::sleep(std::time::Duration::from_millis(10));

    let label = manager.get_label_mut(&label_id).unwrap();
    label.add_contact("test-contact");

    let label = manager.get_label(&label_id).unwrap();
    assert!(label.modified_at() >= prev_modified);
}

/// Tests serialization and deserialization of LabelManager.
#[test]
fn test_label_manager_serialization() {
    let mut manager = LabelManager::new();

    // Create labels with data
    let family_id = manager.create_label("Family").unwrap().id().to_string();
    let friends_id = manager.create_label("Friends").unwrap().id().to_string();

    manager.add_contact_to_label(&family_id, "bob").unwrap();
    manager.add_contact_to_label(&family_id, "carol").unwrap();
    manager.add_contact_to_label(&friends_id, "dave").unwrap();

    let family = manager.get_label_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    manager.set_contact_override("bob", "special-field", true);

    // Serialize
    let json = serde_json::to_string(&manager).unwrap();

    // Deserialize
    let restored: LabelManager = serde_json::from_str(&json).unwrap();

    // Verify restored state
    assert_eq!(restored.label_count(), 2);

    let family = restored.get_label(&family_id).unwrap();
    assert_eq!(family.name(), "Family");
    assert_eq!(family.contact_count(), 2);
    assert!(family.contains_contact("bob"));
    assert!(family.contains_contact("carol"));
    assert!(family.is_field_visible("home-address"));
    assert!(family.is_field_visible("personal-phone"));

    let friends = restored.get_label(&friends_id).unwrap();
    assert_eq!(friends.name(), "Friends");
    assert_eq!(friends.contact_count(), 1);
    assert!(friends.contains_contact("dave"));

    // Verify per-contact override
    assert_eq!(
        restored.get_contact_override("bob", "special-field"),
        Some(true)
    );
}

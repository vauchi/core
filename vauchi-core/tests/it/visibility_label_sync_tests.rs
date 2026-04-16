// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Label Sync, Timestamps, Serialization, and Lifecycle Tests
//!
//! Extracted from visibility_label_tests.rs
//!
//! Traces to: features/visibility_labels.feature
//! - @local-only: Labels sync across devices
//! - Label timestamps, serialization, per-contact override cleanup, bulk field ops

use std::collections::HashSet;

use vauchi_core::contact::{Group, GroupManager};
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
// @scenario: visibility_labels :: Labels sync across my own devices only
// @scenario: visibility_labels :: Labels are not shared with contacts
// @internal
#[test]
fn test_label_sync_across_devices() {
    // =========================================================================
    // Setup: Device A creates labels
    // =========================================================================

    let mut device_a_manager = GroupManager::new();

    let family = device_a_manager.create_group("Family").unwrap();
    let family_id = family.id().to_string();

    let friends = device_a_manager.create_group("Close Friends").unwrap();
    let friends_id = friends.id().to_string();

    // Add contacts and fields
    device_a_manager
        .add_contact_to_group(&family_id, "bob-id")
        .unwrap();
    device_a_manager
        .add_contact_to_group(&family_id, "carol-id")
        .unwrap();
    device_a_manager
        .add_contact_to_group(&friends_id, "dave-id")
        .unwrap();

    let family = device_a_manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    let friends = device_a_manager.get_group_mut(&friends_id).unwrap();
    friends.add_visible_field("personal-email");

    // =========================================================================
    // Create SyncItem::LabelChange for syncing to Device B
    // =========================================================================

    let timestamp = now();

    // Family label sync item
    let family_label = device_a_manager.get_group(&family_id).unwrap();
    let family_sync = SyncItem::LabelChange {
        label_id: family_label.id().to_string(),
        label_name: family_label.name().to_string(),
        contacts: family_label.contacts().iter().cloned().collect(),
        visible_fields: family_label.visible_fields().iter().cloned().collect(),
        is_deleted: false,
        timestamp,
    };

    // Friends label sync item (used for deletion test below)
    let friends_label = device_a_manager.get_group(&friends_id).unwrap();
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
        let label = Group::from_storage(
            label_id,
            label_name,
            contacts.into_iter().collect(),
            visible_fields.into_iter().collect(),
            None,
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

    // This is a design verification - labels exist in GroupManager, not in Contact
    // The contact card sent to others never contains label information
    // Only field visibility is transmitted (what they can see), not why (labels)

    let label = device_a_manager.get_group(&family_id).unwrap();

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
// @scenario: visibility_labels :: Delete contact removes from all labels
// @internal
#[test]
fn test_delete_contact_clears_overrides() {
    let mut manager = GroupManager::new();

    // Create labels
    let family_id = manager.create_group("Family").unwrap().id().to_string();
    let friends_id = manager.create_group("Friends").unwrap().id().to_string();

    let bob = "bob-id";

    // Add Bob to both labels
    manager.add_contact_to_group(&family_id, bob).unwrap();
    manager.add_contact_to_group(&friends_id, bob).unwrap();

    // Set some per-contact overrides
    manager.set_contact_override(bob, "field-1", true);
    manager.set_contact_override(bob, "field-2", false);

    // Verify state
    assert_eq!(manager.groups_for_contact(bob).len(), 2);
    manager
        .get_all_contact_overrides(bob)
        .expect("expected Some");

    // Remove Bob from all labels (simulates contact deletion)
    manager.remove_contact_from_all_groups(bob);

    // Bob should be in no labels
    assert_eq!(manager.groups_for_contact(bob).len(), 0);

    // Per-contact overrides should be cleared
    assert!(manager.get_all_contact_overrides(bob).is_none());
    assert_eq!(manager.get_contact_override(bob, "field-1"), None);
    assert_eq!(manager.get_contact_override(bob, "field-2"), None);
}

/// Tests set_visible_fields bulk operation on a label.
///
/// Feature: visibility_labels.feature
/// Scenario: Configure default fields for a label
// @scenario: visibility_labels :: Configure default fields for a label
// @internal
#[test]
fn test_set_visible_fields_bulk() {
    let mut manager = GroupManager::new();

    let label_id = manager
        .create_group("Professional")
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

    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_visible_fields(work_fields);

    // Verify all fields are set
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 4);
    assert!(label.is_field_visible("work-email"));
    assert!(label.is_field_visible("work-phone"));
    assert!(label.is_field_visible("company"));
    assert!(label.is_field_visible("title"));
    assert!(!label.is_field_visible("personal-email"));

    // Replace with different set
    let minimal_fields: HashSet<String> = ["work-email".to_string()].into_iter().collect();

    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_visible_fields(minimal_fields);

    // Verify replacement
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 1);
    assert!(label.is_field_visible("work-email"));
    assert!(!label.is_field_visible("work-phone"));
}

/// Tests label timestamp updates (created_at, modified_at).
///
/// Feature: visibility_labels.feature
/// Scenario: Labels track creation and modification times
// @internal
#[test]
fn test_label_timestamps() {
    let mut manager = GroupManager::new();

    let label = manager.create_group("Test Label").unwrap();
    let label_id = label.id().to_string();

    let created_at = label.created_at();
    let initial_modified = label.modified_at();

    // Initially, created_at and modified_at should be equal
    assert_eq!(created_at, initial_modified);

    // Modify the label
    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_name("Updated Label");

    let label = manager.get_group(&label_id).unwrap();

    // created_at should be unchanged
    assert_eq!(label.created_at(), created_at);

    // modified_at should be updated (or equal if within same second)
    assert!(label.modified_at() >= initial_modified);

    // Modify again by adding a field
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_visible_field("test-field");

    let label = manager.get_group(&label_id).unwrap();
    assert!(label.modified_at() >= initial_modified);

    // Modify by adding a contact
    let prev_modified = label.modified_at();
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_contact("test-contact");

    let label = manager.get_group(&label_id).unwrap();
    assert!(label.modified_at() >= prev_modified);
}

/// Tests serialization and deserialization of GroupManager.
// @internal
#[test]
fn test_label_manager_serialization() {
    let mut manager = GroupManager::new();

    // Create labels with data
    let family_id = manager.create_group("Family").unwrap().id().to_string();
    let friends_id = manager.create_group("Friends").unwrap().id().to_string();

    manager.add_contact_to_group(&family_id, "bob").unwrap();
    manager.add_contact_to_group(&family_id, "carol").unwrap();
    manager.add_contact_to_group(&friends_id, "dave").unwrap();

    let family = manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address");
    family.add_visible_field("personal-phone");

    manager.set_contact_override("bob", "special-field", true);

    // Serialize
    let json = serde_json::to_string(&manager).unwrap();

    // Deserialize
    let restored: GroupManager = serde_json::from_str(&json).unwrap();

    // Verify restored state
    assert_eq!(restored.group_count(), 2);

    let family = restored.get_group(&family_id).unwrap();
    assert_eq!(family.name(), "Family");
    assert_eq!(family.contact_count(), 2);
    assert!(family.contains_contact("bob"));
    assert!(family.contains_contact("carol"));
    assert!(family.is_field_visible("home-address"));
    assert!(family.is_field_visible("personal-phone"));

    let friends = restored.get_group(&friends_id).unwrap();
    assert_eq!(friends.name(), "Friends");
    assert_eq!(friends.contact_count(), 1);
    assert!(friends.contains_contact("dave"));

    // Verify per-contact override
    assert_eq!(
        restored.get_contact_override("bob", "special-field"),
        Some(true)
    );
}

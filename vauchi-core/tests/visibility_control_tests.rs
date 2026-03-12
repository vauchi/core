// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Control Tests
//!
//! Tests for visibility group management, offline propagation, templates, and bulk operations.
//!
//! Traces to: features/visibility_control.feature
//! - @groups: Group membership changes
//! - @propagation: Sync visibility to offline contacts
//! - @new-contact: Apply predefined visibility templates
//! - @bulk: Batch visibility updates

use std::collections::HashSet;

use vauchi_core::contact::{FieldVisibility, GroupManager, VisibilityRules};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{OfflineQueue, PendingUpdate, Storage, UpdateStatus};

// =============================================================================
// Test Helpers
// =============================================================================

fn test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn create_visibility_update(id: &str, contact_id: &str, field_id: &str) -> PendingUpdate {
    PendingUpdate {
        id: id.to_string(),
        contact_id: contact_id.to_string(),
        update_type: "visibility_change".to_string(),
        payload: field_id.as_bytes().to_vec(),
        created_at: now(),
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    }
}

// =============================================================================
// Group Membership Tests
// =============================================================================
// Traces to: visibility_control.feature @groups scenarios
// - Create a visibility group
// - Apply visibility group to a field
// - Add contact to group updates their visibility
// - Remove contact from group updates their visibility

/// Tests adding and removing contacts from visibility groups (labels).
///
/// Feature: visibility_control.feature
/// Scenarios:
/// - Create a visibility group
/// - Add contact to group updates their visibility
/// - Remove contact from group updates their visibility
// @scenario: visibility_control.feature:Create a visibility group
// @scenario: visibility_control.feature:Add contact to group updates their visibility
// @scenario: visibility_control.feature:Remove contact from group updates their visibility
#[test]
fn test_visibility_group_add_remove() {
    let mut manager = GroupManager::new();

    // Create visibility groups
    let work_label = manager.create_group("Work Contacts").unwrap();
    let work_id = work_label.id().to_string();

    let friends_label = manager.create_group("Close Friends").unwrap();
    let friends_id = friends_label.id().to_string();

    // Add visible fields to each group
    let work_label = manager.get_group_mut(&work_id).unwrap();
    work_label.add_visible_field("work-email");
    work_label.add_visible_field("work-phone");

    let friends_label = manager.get_group_mut(&friends_id).unwrap();
    friends_label.add_visible_field("personal-email");
    friends_label.add_visible_field("home-address");

    // Contacts
    let bob_id = "bob-id";
    let carol_id = "carol-id";
    let dave_id = "dave-id";

    // Initially, no one is in any group
    assert_eq!(manager.can_see_via_labels(bob_id, "work-email"), None);
    assert_eq!(manager.can_see_via_labels(carol_id, "personal-email"), None);

    // Add Bob to Work Contacts
    manager.add_contact_to_group(&work_id, bob_id).unwrap();

    // Bob should now see work fields
    assert_eq!(
        manager.can_see_via_labels(bob_id, "work-email"),
        Some(true),
        "Bob should see work-email after joining Work Contacts"
    );
    assert_eq!(
        manager.can_see_via_labels(bob_id, "work-phone"),
        Some(true),
        "Bob should see work-phone after joining Work Contacts"
    );

    // Bob should NOT see personal fields (not in Close Friends)
    assert_eq!(
        manager.can_see_via_labels(bob_id, "personal-email"),
        None,
        "Bob should not see personal-email (not in Close Friends)"
    );

    // Add Carol to both groups
    manager.add_contact_to_group(&work_id, carol_id).unwrap();
    manager.add_contact_to_group(&friends_id, carol_id).unwrap();

    // Carol should see fields from both groups (union)
    let carol_visible = manager.visible_fields_via_labels(carol_id);
    assert!(carol_visible.contains("work-email"));
    assert!(carol_visible.contains("work-phone"));
    assert!(carol_visible.contains("personal-email"));
    assert!(carol_visible.contains("home-address"));

    // Dave is not in any group
    assert_eq!(
        manager.can_see_via_labels(dave_id, "work-email"),
        None,
        "Dave should not see work-email (not in any group)"
    );
    assert_eq!(
        manager.can_see_via_labels(dave_id, "personal-email"),
        None,
        "Dave should not see personal-email (not in any group)"
    );

    // Remove Bob from Work Contacts
    manager.remove_contact_from_group(&work_id, bob_id).unwrap();

    // Bob should no longer see work fields
    assert_eq!(
        manager.can_see_via_labels(bob_id, "work-email"),
        None,
        "Bob should not see work-email after leaving Work Contacts"
    );

    // Verify label contents
    let work_label = manager.get_group(&work_id).unwrap();
    assert!(!work_label.contains_contact(bob_id));
    assert!(work_label.contains_contact(carol_id));
}

/// Tests that adding a contact to a group grants them visibility to all group fields.
///
/// Feature: visibility_control.feature
/// Scenario: Add contact to group updates their visibility
// @scenario: visibility_control.feature:Add contact to group updates their visibility
#[test]
fn test_visibility_group_grants_all_fields() {
    let mut manager = GroupManager::new();

    // Create a group with multiple fields
    let family_label = manager.create_group("Family").unwrap();
    let family_id = family_label.id().to_string();

    let family_label = manager.get_group_mut(&family_id).unwrap();
    family_label.add_visible_field("home-phone");
    family_label.add_visible_field("home-address");
    family_label.add_visible_field("personal-email");
    family_label.add_visible_field("birthday");

    let contact_id = "family-member-id";

    // Before joining: contact sees no fields via labels
    let visible_before = manager.visible_fields_via_labels(contact_id);
    assert!(
        visible_before.is_empty(),
        "Contact should see no fields before joining group"
    );

    // Add contact to group
    manager
        .add_contact_to_group(&family_id, contact_id)
        .unwrap();

    // After joining: contact sees all group fields
    let visible_after = manager.visible_fields_via_labels(contact_id);
    assert_eq!(
        visible_after.len(),
        4,
        "Contact should see all 4 family fields"
    );
    assert!(visible_after.contains("home-phone"));
    assert!(visible_after.contains("home-address"));
    assert!(visible_after.contains("personal-email"));
    assert!(visible_after.contains("birthday"));
}

/// Tests per-contact override taking precedence over label visibility.
///
/// Feature: visibility_control.feature
/// Scenario: Per-contact override overrides group visibility
// @scenario: visibility_control.feature:Hide a field from a specific contact
#[test]
fn test_visibility_group_with_per_contact_override() {
    let mut manager = GroupManager::new();

    // Create group
    let label = manager.create_group("Friends").unwrap();
    let label_id = label.id().to_string();

    // Add field to group
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_visible_field("personal-phone");

    let contact_id = "special-friend";

    // Add contact to group
    manager.add_contact_to_group(&label_id, contact_id).unwrap();

    // Contact should see the field via group
    assert_eq!(
        manager.can_see_via_labels(contact_id, "personal-phone"),
        Some(true)
    );

    // Set per-contact override to hide the field
    manager.set_contact_override(contact_id, "personal-phone", false);

    // Override takes precedence - contact should NOT see the field
    assert_eq!(
        manager.can_see_via_labels(contact_id, "personal-phone"),
        Some(false),
        "Per-contact override should hide field despite group membership"
    );

    // Remove override
    manager.remove_contact_override(contact_id, "personal-phone");

    // Contact should see field again via group
    assert_eq!(
        manager.can_see_via_labels(contact_id, "personal-phone"),
        Some(true),
        "After removing override, group visibility should apply"
    );
}

// =============================================================================
// Offline Propagation Tests
// =============================================================================
// Traces to: visibility_control.feature @propagation scenarios
// - Visibility change when contact is offline
// - Granting visibility sends update to contact
// - Revoking visibility sends update to contact

/// Tests that visibility changes are queued for offline contacts.
///
/// Feature: visibility_control.feature
/// Scenario: Visibility change when contact is offline
// @scenario: visibility_control.feature:Visibility change when contact is offline
#[test]
fn test_visibility_propagation_offline() {
    let storage = test_storage();

    // Simulate visibility changes for multiple contacts
    let bob_id = "bob-id";
    let carol_id = "carol-id";

    // Queue visibility change updates for offline contacts
    let bob_update = create_visibility_update("update-1", bob_id, "work-email");
    let carol_update = create_visibility_update("update-2", carol_id, "personal-phone");

    storage.queue_update(&bob_update).unwrap();
    storage.queue_update(&carol_update).unwrap();

    // Verify updates are queued
    assert_eq!(
        storage.count_all_pending_updates().unwrap(),
        2,
        "Both visibility updates should be queued"
    );

    // Verify Bob's update is queued
    let bob_updates = storage.get_pending_updates(bob_id).unwrap();
    assert_eq!(bob_updates.len(), 1);
    assert_eq!(bob_updates[0].update_type, "visibility_change");

    // Verify Carol's update is queued
    let carol_updates = storage.get_pending_updates(carol_id).unwrap();
    assert_eq!(carol_updates.len(), 1);

    // Simulate Bob coming online and receiving the update
    storage.mark_update_sent(&bob_update.id).unwrap();

    // Bob's update should be removed, Carol's should remain
    assert_eq!(storage.get_pending_updates(bob_id).unwrap().len(), 0);
    assert_eq!(storage.get_pending_updates(carol_id).unwrap().len(), 1);
    assert_eq!(storage.count_all_pending_updates().unwrap(), 1);
}

/// Tests that multiple visibility changes for the same contact are queued in order.
///
/// Feature: visibility_control.feature
/// Scenario: Multiple visibility changes queue correctly
// @scenario: visibility_control.feature:Visibility change when contact is offline
#[test]
fn test_visibility_propagation_multiple_changes() {
    let storage = test_storage();
    let contact_id = "dave-id";
    let base_time = now();

    // Queue multiple visibility changes with different timestamps
    let updates = vec![
        ("update-3", "work-phone", base_time + 30),
        ("update-1", "work-email", base_time + 10),
        ("update-2", "personal-email", base_time + 20),
    ];

    for (id, field, created_at) in updates {
        let update = PendingUpdate {
            id: id.to_string(),
            contact_id: contact_id.to_string(),
            update_type: "visibility_change".to_string(),
            payload: field.as_bytes().to_vec(),
            created_at,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };
        storage.queue_update(&update).unwrap();
    }

    // Get updates - should be ordered by created_at
    let pending = storage.get_pending_updates(contact_id).unwrap();
    assert_eq!(pending.len(), 3);
    assert_eq!(pending[0].id, "update-1"); // earliest
    assert_eq!(pending[1].id, "update-2");
    assert_eq!(pending[2].id, "update-3"); // latest
}

/// Tests offline queue capacity enforcement.
///
/// Feature: visibility_control.feature
/// Scenario: Queue limits prevent unbounded growth
// @scenario: visibility_control.feature:Visibility change when contact is offline
#[test]
fn test_visibility_propagation_queue_limits() {
    let storage = test_storage();
    let queue = OfflineQueue::with_max_size(5);

    // Fill the queue
    for i in 0..5 {
        let update = create_visibility_update(&format!("update-{}", i), "contact", "field");
        storage.queue_update(&update).unwrap();
    }

    // Queue should be full
    assert!(queue.is_full(&storage).unwrap());
    assert!(!queue.can_queue(&storage).unwrap());
    assert_eq!(queue.remaining_capacity(&storage).unwrap(), 0);

    // Remove one update
    storage.delete_pending_update("update-0").unwrap();

    // Queue should have space again
    assert!(!queue.is_full(&storage).unwrap());
    assert!(queue.can_queue(&storage).unwrap());
    assert_eq!(queue.remaining_capacity(&storage).unwrap(), 1);
}

/// Tests flushing queued updates for a specific contact.
///
/// Feature: visibility_control.feature
/// Scenario: Clear queued updates when contact is deleted/blocked
// @scenario: visibility_control.feature:Delete contact removes their visibility rules
#[test]
fn test_visibility_propagation_flush_contact_queue() {
    let storage = test_storage();

    // Queue updates for multiple contacts
    for i in 0..3 {
        let update = create_visibility_update(&format!("alice-{}", i), "alice", "field");
        storage.queue_update(&update).unwrap();
    }
    for i in 0..2 {
        let update = create_visibility_update(&format!("bob-{}", i), "bob", "field");
        storage.queue_update(&update).unwrap();
    }

    assert_eq!(storage.count_all_pending_updates().unwrap(), 5);

    // Flush all updates for Alice (e.g., if Alice is blocked)
    let deleted = storage.delete_pending_updates_for_contact("alice").unwrap();
    assert_eq!(deleted, 3);

    // Only Bob's updates remain
    assert_eq!(storage.count_all_pending_updates().unwrap(), 2);
    assert_eq!(storage.get_pending_updates("alice").unwrap().len(), 0);
    assert_eq!(storage.get_pending_updates("bob").unwrap().len(), 2);
}

// =============================================================================
// Visibility Templates Tests
// =============================================================================
// Traces to: visibility_control.feature @new-contact scenarios
// - Apply template visibility to new contact
// - Set visibility before exchange

/// Tests applying predefined visibility templates to contacts.
///
/// Feature: visibility_control.feature
/// Scenario: Apply template visibility to new contact
// @scenario: visibility_control.feature:Apply template visibility to new contact
#[test]
fn test_visibility_templates() {
    // Define visibility templates as predefined VisibilityRules configurations

    // Template: Professional (work fields only)
    fn professional_template() -> VisibilityRules {
        let mut rules = VisibilityRules::new();
        // Work fields are visible by default (Everyone)
        rules.set_everyone("work-email");
        rules.set_everyone("work-phone");
        // Personal fields are hidden
        rules.set_nobody("personal-email");
        rules.set_nobody("personal-phone");
        rules.set_nobody("home-address");
        rules
    }

    // Template: Personal (all fields except work)
    fn personal_template() -> VisibilityRules {
        let mut rules = VisibilityRules::new();
        rules.set_everyone("personal-email");
        rules.set_everyone("personal-phone");
        rules.set_everyone("home-address");
        rules.set_nobody("work-email");
        rules.set_nobody("work-phone");
        rules
    }

    // Template: Minimal (only name, nothing else)
    fn minimal_template() -> VisibilityRules {
        let mut rules = VisibilityRules::new();
        rules.set_nobody("work-email");
        rules.set_nobody("work-phone");
        rules.set_nobody("personal-email");
        rules.set_nobody("personal-phone");
        rules.set_nobody("home-address");
        rules
    }

    let contact_id = "new-contact";

    // Apply Professional template
    let professional = professional_template();
    assert!(
        professional.can_see("work-email", contact_id),
        "Professional template: work-email should be visible"
    );
    assert!(
        professional.can_see("work-phone", contact_id),
        "Professional template: work-phone should be visible"
    );
    assert!(
        !professional.can_see("personal-email", contact_id),
        "Professional template: personal-email should be hidden"
    );
    assert!(
        !professional.can_see("personal-phone", contact_id),
        "Professional template: personal-phone should be hidden"
    );
    assert!(
        !professional.can_see("home-address", contact_id),
        "Professional template: home-address should be hidden"
    );

    // Apply Personal template
    let personal = personal_template();
    assert!(
        !personal.can_see("work-email", contact_id),
        "Personal template: work-email should be hidden"
    );
    assert!(
        personal.can_see("personal-email", contact_id),
        "Personal template: personal-email should be visible"
    );
    assert!(
        personal.can_see("home-address", contact_id),
        "Personal template: home-address should be visible"
    );

    // Apply Minimal template
    let minimal = minimal_template();
    assert!(
        !minimal.can_see("work-email", contact_id),
        "Minimal template: work-email should be hidden"
    );
    assert!(
        !minimal.can_see("personal-email", contact_id),
        "Minimal template: personal-email should be hidden"
    );
    assert!(
        !minimal.can_see("home-address", contact_id),
        "Minimal template: home-address should be hidden"
    );
}

/// Tests applying a template to specific contacts.
///
/// Feature: visibility_control.feature
/// Scenario: Apply different templates to different contacts
// @scenario: visibility_control.feature:Apply template visibility to new contact
// @scenario: visibility_control.feature:View what a specific contact can see
#[test]
fn test_visibility_templates_per_contact() {
    let all_fields = vec![
        "work-email",
        "work-phone",
        "personal-email",
        "personal-phone",
    ];

    // Create rules for Bob (professional contact)
    let mut bob_rules = VisibilityRules::new();
    bob_rules.set_everyone("work-email");
    bob_rules.set_everyone("work-phone");
    bob_rules.set_nobody("personal-email");
    bob_rules.set_nobody("personal-phone");

    // Create rules for Carol (friend)
    let mut carol_rules = VisibilityRules::new();
    carol_rules.set_everyone("personal-email");
    carol_rules.set_everyone("personal-phone");
    carol_rules.set_nobody("work-email");
    carol_rules.set_nobody("work-phone");

    // Get visible fields for each
    let bob_visible = bob_rules.visible_fields("bob", &all_fields);
    let carol_visible = carol_rules.visible_fields("carol", &all_fields);

    assert_eq!(bob_visible.len(), 2);
    assert!(bob_visible.contains(&"work-email".to_string()));
    assert!(bob_visible.contains(&"work-phone".to_string()));

    assert_eq!(carol_visible.len(), 2);
    assert!(carol_visible.contains(&"personal-email".to_string()));
    assert!(carol_visible.contains(&"personal-phone".to_string()));
}

/// Tests template with specific contacts list.
///
/// Feature: visibility_control.feature
/// Scenario: Show a field only to specific contacts
// @scenario: visibility_control.feature:Show a field only to specific contacts
// @scenario: visibility_control.feature:Visibility audit shows all contacts for a field
#[test]
fn test_visibility_templates_with_contacts_list() {
    let mut rules = VisibilityRules::new();

    // Make work-email visible only to Bob and Carol
    let mut allowed = HashSet::new();
    allowed.insert("bob-id".to_string());
    allowed.insert("carol-id".to_string());
    rules.set_contacts("work-email", allowed);

    // Make personal-phone visible to everyone
    rules.set_everyone("personal-phone");

    // Make home-address private
    rules.set_nobody("home-address");

    // Test visibility
    assert!(
        rules.can_see("work-email", "bob-id"),
        "Bob should see work-email"
    );
    assert!(
        rules.can_see("work-email", "carol-id"),
        "Carol should see work-email"
    );
    assert!(
        !rules.can_see("work-email", "dave-id"),
        "Dave should not see work-email"
    );

    assert!(
        rules.can_see("personal-phone", "anyone"),
        "Anyone should see personal-phone"
    );

    assert!(
        !rules.can_see("home-address", "bob-id"),
        "Bob should not see home-address"
    );
}

// =============================================================================
// Bulk Operations Tests
// =============================================================================
// Traces to: visibility_control.feature @bulk scenarios
// - Set visibility for all fields at once
// - Reset all visibility to default

/// Tests setting visibility for all fields at once.
///
/// Feature: visibility_control.feature
/// Scenario: Set visibility for all fields at once
// @scenario: visibility_control.feature:Set visibility for all fields at once
#[test]
fn test_visibility_bulk_operations() {
    let fields = vec![
        "field-1", "field-2", "field-3", "field-4", "field-5", "field-6", "field-7", "field-8",
        "field-9", "field-10",
    ];

    // Bulk operation: Set all fields to visible only for Bob
    let mut rules = VisibilityRules::new();
    let mut bob_only = HashSet::new();
    bob_only.insert("bob-id".to_string());

    for field in &fields {
        rules.set_contacts(field, bob_only.clone());
    }

    // Verify all 10 fields are visible only to Bob
    for field in &fields {
        assert!(rules.can_see(field, "bob-id"), "Bob should see {}", field);
        assert!(
            !rules.can_see(field, "carol-id"),
            "Carol should not see {}",
            field
        );
        assert!(
            !rules.can_see(field, "dave-id"),
            "Dave should not see {}",
            field
        );
    }

    // Verify Bob sees all 10 fields
    let bob_visible = rules.visible_fields("bob-id", &fields);
    assert_eq!(bob_visible.len(), 10, "Bob should see all 10 fields");

    // Verify Carol and Dave see no fields
    let carol_visible = rules.visible_fields("carol-id", &fields);
    assert_eq!(carol_visible.len(), 0, "Carol should see no fields");

    let dave_visible = rules.visible_fields("dave-id", &fields);
    assert_eq!(dave_visible.len(), 0, "Dave should see no fields");
}

/// Tests resetting all visibility to default.
///
/// Feature: visibility_control.feature
/// Scenario: Reset all visibility to default
// @scenario: visibility_control.feature:Reset all visibility to default
#[test]
fn test_visibility_bulk_reset_to_default() {
    let fields = vec!["field-1", "field-2", "field-3", "field-4", "field-5"];

    // Set custom visibility for various fields
    let mut rules = VisibilityRules::new();
    rules.set_nobody("field-1");
    rules.set_nobody("field-2");

    let mut some_contacts = HashSet::new();
    some_contacts.insert("alice".to_string());
    rules.set_contacts("field-3", some_contacts);

    // Verify custom rules are in effect
    assert!(!rules.can_see("field-1", "anyone"));
    assert!(!rules.can_see("field-2", "anyone"));
    assert!(!rules.can_see("field-3", "bob"));
    assert!(rules.can_see("field-3", "alice"));

    // Bulk reset: Remove all rules to revert to default
    for field in &fields {
        rules.remove(field);
    }

    // After reset, all fields should be visible to everyone (default)
    for field in &fields {
        assert_eq!(
            *rules.get(field),
            FieldVisibility::Everyone,
            "{} should have default visibility",
            field
        );
        assert!(
            rules.can_see(field, "anyone"),
            "Anyone should see {} after reset",
            field
        );
    }
}

/// Tests bulk operations with label manager.
///
/// Feature: visibility_control.feature
/// Scenario: Bulk add/remove fields from a group
// @scenario: visibility_control.feature:Apply visibility group to a field
#[test]
fn test_visibility_bulk_label_operations() {
    let mut manager = GroupManager::new();

    // Create a label
    let label = manager.create_group("Work").unwrap();
    let label_id = label.id().to_string();

    // Bulk add multiple fields to the label
    let work_fields = vec![
        "work-email",
        "work-phone",
        "work-address",
        "work-title",
        "company-name",
    ];

    let label = manager.get_group_mut(&label_id).unwrap();
    for field in &work_fields {
        label.add_visible_field(field);
    }

    // Verify all fields are in the label
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 5);
    for field in &work_fields {
        assert!(
            label.is_field_visible(field),
            "{} should be visible in Work label",
            field
        );
    }

    // Bulk add multiple contacts to the label
    let contacts = vec!["alice", "bob", "carol", "dave"];
    for contact in &contacts {
        manager.add_contact_to_group(&label_id, contact).unwrap();
    }

    // Verify all contacts see all work fields
    for contact in &contacts {
        let visible = manager.visible_fields_via_labels(contact);
        assert_eq!(visible.len(), 5, "{} should see all 5 work fields", contact);
    }

    // Bulk remove fields from the label
    let label = manager.get_group_mut(&label_id).unwrap();
    for field in &["work-address", "company-name"] {
        label.remove_visible_field(field);
    }

    // Verify fields were removed
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 3);
    assert!(!label.is_field_visible("work-address"));
    assert!(!label.is_field_visible("company-name"));

    // Contacts should now see only 3 fields
    for contact in &contacts {
        let visible = manager.visible_fields_via_labels(contact);
        assert_eq!(visible.len(), 3, "{} should see 3 work fields", contact);
    }
}

/// Tests bulk clearing of contact overrides.
///
/// Feature: visibility_control.feature
/// Scenario: Clear all per-contact overrides
// @scenario: visibility_control.feature:Hide a field from a specific contact
#[test]
fn test_visibility_bulk_clear_overrides() {
    let mut manager = GroupManager::new();

    let contact_id = "special-contact";

    // Set multiple per-contact overrides
    manager.set_contact_override(contact_id, "field-1", true);
    manager.set_contact_override(contact_id, "field-2", false);
    manager.set_contact_override(contact_id, "field-3", true);
    manager.set_contact_override(contact_id, "field-4", false);

    // Verify overrides are in effect
    assert_eq!(
        manager.get_contact_override(contact_id, "field-1"),
        Some(true)
    );
    assert_eq!(
        manager.get_contact_override(contact_id, "field-2"),
        Some(false)
    );

    // Bulk clear all overrides for the contact
    manager.clear_contact_overrides(contact_id);

    // Verify all overrides are gone
    assert_eq!(manager.get_contact_override(contact_id, "field-1"), None);
    assert_eq!(manager.get_contact_override(contact_id, "field-2"), None);
    assert_eq!(manager.get_contact_override(contact_id, "field-3"), None);
    assert_eq!(manager.get_contact_override(contact_id, "field-4"), None);

    // Verify there are no overrides for this contact
    assert!(manager.get_all_contact_overrides(contact_id).is_none());
}

/// Tests removing a contact from all labels at once.
///
/// Feature: visibility_control.feature
/// Scenario: Delete contact removes their visibility rules
// @scenario: visibility_control.feature:Delete contact removes their visibility rules
#[test]
fn test_visibility_bulk_remove_contact_from_all_labels() {
    let mut manager = GroupManager::new();

    // Create multiple labels
    let family = manager.create_group("Family").unwrap().id().to_string();
    let friends = manager.create_group("Friends").unwrap().id().to_string();
    let work = manager.create_group("Work").unwrap().id().to_string();

    let contact_id = "departing-contact";

    // Add contact to all labels
    manager.add_contact_to_group(&family, contact_id).unwrap();
    manager.add_contact_to_group(&friends, contact_id).unwrap();
    manager.add_contact_to_group(&work, contact_id).unwrap();

    // Also set some per-contact overrides
    manager.set_contact_override(contact_id, "special-field", true);

    // Verify contact is in all labels
    let contact_labels = manager.labels_for_contact(contact_id);
    assert_eq!(contact_labels.len(), 3);

    // Bulk remove contact from all labels (e.g., when deleting the contact)
    manager.remove_contact_from_all_groups(contact_id);

    // Verify contact is removed from all labels
    let contact_labels = manager.labels_for_contact(contact_id);
    assert_eq!(contact_labels.len(), 0, "Contact should be in no labels");

    // Verify per-contact overrides are also cleared
    assert!(
        manager.get_all_contact_overrides(contact_id).is_none(),
        "Per-contact overrides should be cleared"
    );
}

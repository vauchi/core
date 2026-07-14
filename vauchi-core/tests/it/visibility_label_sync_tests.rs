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

use vauchi_core::Vauchi;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::contact::GroupManager;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::{GroupSyncData, SyncItem};

// =============================================================================
// =============================================================================

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn tiny_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode test PNG");
    bytes.into_inner()
}

// =============================================================================
// Test 5: Label Sync Across Devices
// =============================================================================
// Traces to: visibility_labels.feature
// - @local-only: Labels sync across my own devices only
// - @local-only: Labels are not shared with contacts

// @scenario: device_management :: Group presentation changes sync to linked devices
// @scenario: sync_updates :: Group presentation state converges across linked devices
#[test]
fn group_mutations_journal_complete_state_for_linked_devices() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    const SEED: [u8; 32] = [7u8; 32];
    let signing = SigningKeyPair::from_seed(&SEED);
    let mut registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &signing,
    );
    let tablet = DeviceInfo::derive(&SEED, 1, "tablet".into(), 0);
    let tablet_id = *tablet.device_id();
    registry
        .add_device_unsigned(tablet.to_registered(&SEED))
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();

    let created = vauchi.create_group("Work").unwrap();
    vauchi
        .add_contact_to_group(created.id(), "contact-bob")
        .unwrap();
    vauchi
        .set_group_field_visibility(created.id(), "field-email", true)
        .unwrap();
    vauchi
        .set_group_display_name_override(created.id(), Some("Alice at Work"))
        .unwrap();
    vauchi
        .set_group_bio_override(created.id(), Some("Professional profile"))
        .unwrap();
    vauchi
        .set_group_avatar_override(created.id(), Some(&tiny_png()))
        .unwrap();
    let expected = vauchi.get_group(created.id()).unwrap();

    let orchestrator = DeviceSyncOrchestrator::load(
        vauchi.storage(),
        vauchi.identity().unwrap().create_device_info(0),
        registry.clone(),
    )
    .unwrap();
    let queued: Vec<_> = orchestrator
        .pending_for_device(&tablet_id)
        .iter()
        .filter_map(|item| match item {
            SyncItem::GroupChanged { group_data, .. } => Some(group_data),
            _ => None,
        })
        .collect();
    assert_eq!(queued.len(), 6, "every group mutation must be journaled");
    let queued = queued.last().unwrap();

    assert_eq!(queued.id, expected.id());
    assert_eq!(queued.name, "Work");
    assert_eq!(queued.contact_ids, ["contact-bob"]);
    assert_eq!(queued.visible_fields, ["field-email"]);
    assert_eq!(
        queued.display_name_override.as_deref(),
        Some("Alice at Work")
    );
    assert_eq!(queued.bio_override.as_deref(), Some("Professional profile"));
    assert_eq!(
        queued.avatar_override.as_deref(),
        expected.avatar_override()
    );
    assert_eq!(queued.created_at, expected.created_at());
    assert_eq!(queued.modified_at, expected.modified_at());

    vauchi.delete_group(expected.id()).unwrap();
    let orchestrator = DeviceSyncOrchestrator::load(
        vauchi.storage(),
        vauchi.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap();
    assert!(matches!(
        orchestrator.pending_for_device(&tablet_id).last(),
        Some(SyncItem::GroupDeleted { group_id, .. }) if group_id == expected.id()
    ));
}

/// Tests that groups sync across the user's own devices without entering contact data.
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
    let mut device_a_manager = GroupManager::new();

    let family = device_a_manager.create_group("Family", 0).unwrap();
    let family_id = family.id().to_string();

    let friends = device_a_manager.create_group("Close Friends", 0).unwrap();
    let friends_id = friends.id().to_string();

    device_a_manager
        .add_contact_to_group(&family_id, "bob-id", 0)
        .unwrap();
    device_a_manager
        .add_contact_to_group(&family_id, "carol-id", 0)
        .unwrap();
    device_a_manager
        .add_contact_to_group(&friends_id, "dave-id", 0)
        .unwrap();

    let family = device_a_manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address", 0);
    family.add_visible_field("personal-phone", 0);

    let timestamp = now();
    let family_data = GroupSyncData::from_group(device_a_manager.get_group(&family_id).unwrap());
    let family_sync = SyncItem::GroupChanged {
        group_data: family_data.clone(),
        timestamp,
    };

    if let SyncItem::GroupChanged {
        group_data,
        timestamp: ts,
    } = &family_sync
    {
        assert_eq!(group_data.id, family_id);
        assert_eq!(group_data.name, "Family");
        assert_eq!(group_data.contact_ids, ["bob-id", "carol-id"]);
        assert_eq!(
            group_data.visible_fields,
            ["home-address", "personal-phone"]
        );
        assert_eq!(*ts, timestamp);
    } else {
        panic!("Expected GroupChanged variant");
    }

    let reconstructed = family_data.to_group();
    assert_eq!(reconstructed.name(), "Family");
    assert_eq!(reconstructed.contact_count(), 2);
    assert!(reconstructed.contains_contact("bob-id"));
    assert!(reconstructed.contains_contact("carol-id"));
    assert!(reconstructed.is_field_visible("home-address"));
    assert!(reconstructed.is_field_visible("personal-phone"));

    let deleted_sync = SyncItem::GroupDeleted {
        group_id: friends_id.clone(),
        timestamp: timestamp + 1,
    };
    assert!(matches!(
        deleted_sync,
        SyncItem::GroupDeleted { group_id, .. } if group_id == friends_id
    ));

    let mut older_data = family_data.clone();
    older_data.name = "Family Members".to_string();
    older_data.contact_ids = vec!["bob-id".to_string()];
    let item_a = SyncItem::GroupChanged {
        group_data: older_data,
        timestamp: 1000,
    };
    let mut newer_data = family_data;
    newer_data.name = "Close Family".to_string();
    newer_data.visible_fields.push("birthday".to_string());
    let item_b = SyncItem::GroupChanged {
        group_data: newer_data,
        timestamp: 2000,
    };

    let device_a_id = [0xAA; 32];
    let device_b_id = [0xBB; 32];

    let resolved = SyncItem::resolve_conflict(&item_a, &item_b, &device_a_id, &device_b_id);

    if let SyncItem::GroupChanged {
        group_data,
        timestamp: ts,
    } = resolved
    {
        assert_eq!(group_data.name, "Close Family");
        assert_eq!(group_data.contact_ids.len(), 2);
        assert_eq!(group_data.visible_fields.len(), 3);
        assert_eq!(ts, 2000);
    } else {
        panic!("Expected GroupChanged variant");
    }

    // Verify labels are local-only (not transmitted to contacts)

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

    let family_id = manager.create_group("Family", 0).unwrap().id().to_string();
    let friends_id = manager.create_group("Friends", 0).unwrap().id().to_string();

    let bob = "bob-id";

    manager.add_contact_to_group(&family_id, bob, 0).unwrap();
    manager.add_contact_to_group(&friends_id, bob, 0).unwrap();

    manager.set_contact_override(bob, "field-1", true);
    manager.set_contact_override(bob, "field-2", false);

    assert_eq!(manager.groups_for_contact(bob).len(), 2);
    manager
        .get_all_contact_overrides(bob)
        .expect("expected Some");

    // Remove Bob from all labels (simulates contact deletion)
    manager.remove_contact_from_all_groups(bob, 0);

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
        .create_group("Professional", 0)
        .unwrap()
        .id()
        .to_string();

    let work_fields: HashSet<String> = [
        "work-email".to_string(),
        "work-phone".to_string(),
        "company".to_string(),
        "title".to_string(),
    ]
    .into_iter()
    .collect();

    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_visible_fields(work_fields, 0);

    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.visible_fields().len(), 4);
    assert!(label.is_field_visible("work-email"));
    assert!(label.is_field_visible("work-phone"));
    assert!(label.is_field_visible("company"));
    assert!(label.is_field_visible("title"));
    assert!(!label.is_field_visible("personal-email"));

    let minimal_fields: HashSet<String> = ["work-email".to_string()].into_iter().collect();

    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_visible_fields(minimal_fields, 0);

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
    // Caller-controlled `now` lets us pin each mutation's
    // timestamp deterministically. The original assertions
    // (`modified_at >= initial_modified`) trivially held under
    // ambient `SystemTime::now()` because real wall-clock time
    // advances monotonically; here we use distinct values to
    // assert that the mutator-stamping mechanism actually runs.
    let mut manager = GroupManager::new();

    let label = manager.create_group("Test Label", 1000).unwrap();
    let label_id = label.id().to_string();

    // new() stamps both created_at and modified_at from the same `now`.
    assert_eq!(label.created_at(), 1000);
    assert_eq!(label.modified_at(), 1000);

    // set_name updates modified_at, leaves created_at.
    let label = manager.get_group_mut(&label_id).unwrap();
    label.set_name("Updated Label", 1100);
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(
        label.created_at(),
        1000,
        "created_at must be immutable after construction"
    );
    assert_eq!(
        label.modified_at(),
        1100,
        "set_name must update modified_at"
    );

    // add_visible_field updates modified_at.
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_visible_field("test-field", 1200);
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.modified_at(), 1200);

    // add_contact updates modified_at.
    let label = manager.get_group_mut(&label_id).unwrap();
    label.add_contact("test-contact", 1300);
    let label = manager.get_group(&label_id).unwrap();
    assert_eq!(label.modified_at(), 1300);
}

/// Tests serialization and deserialization of GroupManager.
// @internal
#[test]
fn test_label_manager_serialization() {
    let mut manager = GroupManager::new();

    let family_id = manager.create_group("Family", 0).unwrap().id().to_string();
    let friends_id = manager.create_group("Friends", 0).unwrap().id().to_string();

    manager.add_contact_to_group(&family_id, "bob", 0).unwrap();
    manager
        .add_contact_to_group(&family_id, "carol", 0)
        .unwrap();
    manager
        .add_contact_to_group(&friends_id, "dave", 0)
        .unwrap();

    let family = manager.get_group_mut(&family_id).unwrap();
    family.add_visible_field("home-address", 0);
    family.add_visible_field("personal-phone", 0);

    manager.set_contact_override("bob", "special-field", true);

    let json = serde_json::to_string(&manager).unwrap();

    let restored: GroupManager = serde_json::from_str(&json).unwrap();

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

    assert_eq!(
        restored.get_contact_override("bob", "special-field"),
        Some(true)
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::state
//! Extracted from state.rs

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::*;
use vauchi_core::*;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

// @scenario: sync_updates :: Update propagates to online contacts
#[test]
fn test_sync_queue_card_update() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let update_id = manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();
    assert!(!update_id.is_empty());

    let pending = manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].update_type, "card_update");
}

// @scenario: sync_updates :: Only changed fields transmitted
#[test]
fn test_sync_no_changes() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let card = ContactCard::new("Alice");

    let result = manager.queue_card_update("contact-1", &card, &card);
    assert!(matches!(result, Err(SyncError::NoChanges)));
}

// @scenario: sync_updates :: Update only visible fields
#[test]
fn test_sync_queue_visibility_change() {
    let storage = create_test_storage();
    let manager = SyncManager::new(&storage);

    let update_id = manager
        .queue_visibility_change("contact-1", vec!["email".to_string(), "phone".to_string()])
        .unwrap();

    assert!(!update_id.is_empty());

    let pending = manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].update_type, "visibility_change");
}

// @scenario: sync_updates :: Queued updates delivered when contact comes online
#[test]
fn test_sync_mark_delivered() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let update_id = manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();

    assert_eq!(manager.get_pending("contact-1").unwrap().len(), 1);

    manager.mark_delivered(&update_id).unwrap();

    assert_eq!(manager.get_pending("contact-1").unwrap().len(), 0);
}

// @scenario: sync_updates :: Retry failed sync with exponential backoff
#[test]
fn test_sync_mark_failed_with_backoff() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let update_id = manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();

    manager
        .mark_failed(&update_id, "Connection refused", 0)
        .unwrap();

    let pending = manager.get_pending("contact-1").unwrap();
    assert!(matches!(pending[0].status, UpdateStatus::Failed { .. }));
}

// @scenario: sync_updates :: Update queued for offline contacts
#[test]
fn test_sync_state_pending() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();

    let state = manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(
        state,
        SyncState::Pending {
            queued_count: 1,
            ..
        }
    ));
}

// @scenario: sync_updates :: View sync status for all contacts
#[test]
fn test_sync_state_synced() {
    let storage = create_test_storage();
    let manager = SyncManager::new(&storage);

    let state = manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, SyncState::Synced { .. }));
}

// @scenario: sync_updates :: Retry failed sync with exponential backoff
#[test]
fn test_sync_state_failed() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    let update_id = manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();
    manager.mark_failed(&update_id, "Network error", 0).unwrap();

    let state = manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, SyncState::Failed { .. }));
}

// @scenario: sync_updates :: Multiple updates coalesced for offline contact
#[test]
fn test_sync_coalesce_updates() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    // Queue multiple updates
    let card1 = ContactCard::new("Alice");
    let mut card2 = ContactCard::new("Alice");
    let _ = card2.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));
    let mut card3 = ContactCard::new("Alice");
    let _ = card3.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));
    let _ = card3.add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"));

    manager
        .queue_card_update("contact-1", &card1, &card2)
        .unwrap();
    manager
        .queue_card_update("contact-1", &card2, &card3)
        .unwrap();

    assert_eq!(manager.get_pending("contact-1").unwrap().len(), 2);

    // Coalesce
    let merged_id = manager.coalesce_updates("contact-1").unwrap();
    merged_id.expect("expected Some");

    // Should now have only one update
    assert_eq!(manager.get_pending("contact-1").unwrap().len(), 1);
}

// @scenario: sync_updates :: View sync status for all contacts
#[test]
fn test_sync_status_multiple_contacts() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let card1 = ContactCard::new("Alice");
    let mut card2 = ContactCard::new("Alice");
    let _ = card2.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));

    manager
        .queue_card_update("contact-1", &card1, &card2)
        .unwrap();
    manager
        .queue_card_update("contact-2", &card1, &card2)
        .unwrap();

    let status = manager.sync_status().unwrap();

    assert_eq!(status.len(), 2);
    assert!(status.contains_key("contact-1"));
    assert!(status.contains_key("contact-2"));
}

/// Test: last_sync timestamp is properly tracked after update delivery
// @scenario: sync_updates :: View detailed sync status for a contact
#[test]
fn test_sync_state_tracks_last_sync_timestamp() {
    let storage = create_test_storage();
    let mut manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    let _ = old_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
    ));

    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
    ));

    // Get current timestamp before the update
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Queue and deliver an update
    let update_id = manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();
    manager.mark_delivered(&update_id).unwrap();

    // Get sync state - should have a non-zero last_sync that's recent
    let state = manager.get_sync_state("contact-1").unwrap();

    match state {
        SyncState::Synced { last_sync } => {
            // last_sync should be non-zero (not the placeholder value)
            assert!(last_sync > 0, "last_sync should be non-zero after delivery");

            // last_sync should be within the last minute (reasonable test window)
            assert!(
                last_sync >= now - 60 && last_sync <= now + 60,
                "last_sync should be a recent timestamp, got {} (expected around {})",
                last_sync,
                now
            );
        }
        other => panic!("Expected SyncState::Synced, got {:?}", other),
    }
}

// === Grouped by Relay Tests ===

// @scenario: sync_updates :: Route updates to correct relay per contact
#[test]
fn test_sync_get_ready_grouped_by_relay() {
    let storage = create_test_storage();
    let manager = SyncManager::new(&storage);

    // Queue updates with different relay URLs directly via storage
    let updates = vec![
        ("u1", "alice", Some("https://relay-a.example.com")),
        ("u2", "bob", Some("https://relay-a.example.com")),
        ("u3", "carol", Some("https://relay-b.example.com")),
        ("u4", "dave", None),
    ];

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    for (id, contact_id, relay_url) in updates {
        let update = PendingUpdate {
            id: id.to_string(),
            contact_id: contact_id.to_string(),
            update_type: "card_delta".to_string(),
            payload: vec![1, 2, 3],
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: relay_url.map(String::from),
        };
        storage.queue_update(&update).unwrap();
    }

    let grouped = manager.get_ready_grouped_by_relay().unwrap();

    // 3 groups: relay-a (2), relay-b (1), None/home (1)
    assert_eq!(grouped.len(), 3);

    let relay_a = grouped
        .get(&Some("https://relay-a.example.com".to_string()))
        .unwrap();
    assert_eq!(relay_a.len(), 2);

    let relay_b = grouped
        .get(&Some("https://relay-b.example.com".to_string()))
        .unwrap();
    assert_eq!(relay_b.len(), 1);

    let home = grouped.get(&None).unwrap();
    assert_eq!(home.len(), 1);
}

// @scenario: sync_updates :: Failed updates not ready are excluded from grouped query
#[test]
fn test_sync_grouped_excludes_not_ready_failed() {
    let storage = create_test_storage();
    let manager = SyncManager::new(&storage);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // One pending (ready)
    let u1 = PendingUpdate {
        id: "u1".to_string(),
        contact_id: "alice".to_string(),
        update_type: "card_delta".to_string(),
        payload: vec![1],
        created_at: now,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: Some("https://relay.example.com".to_string()),
    };

    // One failed with retry in the future (not ready)
    let u2 = PendingUpdate {
        id: "u2".to_string(),
        contact_id: "bob".to_string(),
        update_type: "card_delta".to_string(),
        payload: vec![2],
        created_at: now,
        retry_count: 1,
        status: UpdateStatus::Failed {
            error: "timeout".to_string(),
            retry_at: now + 9999,
        },
        target_relay_url: Some("https://relay.example.com".to_string()),
    };

    storage.queue_update(&u1).unwrap();
    storage.queue_update(&u2).unwrap();

    let grouped = manager.get_ready_grouped_by_relay().unwrap();

    // Only the pending one should be in the result
    let relay = grouped
        .get(&Some("https://relay.example.com".to_string()))
        .unwrap();
    assert_eq!(relay.len(), 1);
    assert_eq!(relay[0].id, "u1");
}

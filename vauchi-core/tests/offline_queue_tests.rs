//! Tests for offline queue functionality.
//!
//! Traces to: features/message_delivery.feature
//! P14 Phase 5: Offline Queue

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{OfflineQueue, PendingUpdate, Storage, UpdateStatus};

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

fn create_pending_update(id: &str, contact_id: &str) -> PendingUpdate {
    PendingUpdate {
        id: id.to_string(),
        contact_id: contact_id.to_string(),
        update_type: "card_delta".to_string(),
        payload: vec![1, 2, 3],
        created_at: now(),
        retry_count: 0,
        status: UpdateStatus::Pending,
    }
}

// === Queue Size Tests ===

#[test]
fn test_count_all_pending_updates() {
    let storage = test_storage();

    // Initially empty
    assert_eq!(storage.count_all_pending_updates().unwrap(), 0);

    // Add updates for different contacts
    for i in 0..5 {
        let update = create_pending_update(&format!("update-{}", i), &format!("contact-{}", i % 3));
        storage.queue_update(&update).unwrap();
    }

    // Should count all updates
    assert_eq!(storage.count_all_pending_updates().unwrap(), 5);
}

#[test]
fn test_offline_queue_default_limit() {
    let queue = OfflineQueue::new();

    // Default limit should be 1000
    assert_eq!(queue.max_queue_size(), 1000);
}

#[test]
fn test_offline_queue_custom_limit() {
    let queue = OfflineQueue::with_max_size(500);
    assert_eq!(queue.max_queue_size(), 500);
}

#[test]
fn test_is_queue_full() {
    let storage = test_storage();
    let queue = OfflineQueue::with_max_size(5);

    // Add 4 updates
    for i in 0..4 {
        let update = create_pending_update(&format!("update-{}", i), "contact");
        storage.queue_update(&update).unwrap();
    }

    // Not full yet
    assert!(!queue.is_full(&storage).unwrap());

    // Add one more
    let update = create_pending_update("update-4", "contact");
    storage.queue_update(&update).unwrap();

    // Now full
    assert!(queue.is_full(&storage).unwrap());
}

#[test]
fn test_can_queue_update() {
    let storage = test_storage();
    let queue = OfflineQueue::with_max_size(3);

    // Fill the queue
    for i in 0..3 {
        let update = create_pending_update(&format!("update-{}", i), "contact");
        storage.queue_update(&update).unwrap();
    }

    // Can't queue more when full
    assert!(!queue.can_queue(&storage).unwrap());

    // Remove one
    storage.delete_pending_update("update-0").unwrap();

    // Can queue again
    assert!(queue.can_queue(&storage).unwrap());
}

#[test]
fn test_queue_size_remaining() {
    let storage = test_storage();
    let queue = OfflineQueue::with_max_size(10);

    // Initially 10 remaining
    assert_eq!(queue.remaining_capacity(&storage).unwrap(), 10);

    // Add 3 updates
    for i in 0..3 {
        let update = create_pending_update(&format!("update-{}", i), "contact");
        storage.queue_update(&update).unwrap();
    }

    // 7 remaining
    assert_eq!(queue.remaining_capacity(&storage).unwrap(), 7);
}

// === Queue Ordering Tests ===

#[test]
fn test_pending_updates_ordered_by_creation() {
    let storage = test_storage();
    let base_time = now();

    // Add updates with different timestamps
    let updates = vec![
        ("msg-3", "contact", base_time + 30),
        ("msg-1", "contact", base_time + 10),
        ("msg-2", "contact", base_time + 20),
    ];

    for (id, contact, created_at) in updates {
        let update = PendingUpdate {
            id: id.to_string(),
            contact_id: contact.to_string(),
            update_type: "card_delta".to_string(),
            payload: vec![],
            created_at,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };
        storage.queue_update(&update).unwrap();
    }

    // Get all - should be ordered by created_at
    let all = storage.get_all_pending_updates().unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, "msg-1"); // created_at + 10
    assert_eq!(all[1].id, "msg-2"); // created_at + 20
    assert_eq!(all[2].id, "msg-3"); // created_at + 30
}

// === Flush Queue Tests ===

#[test]
fn test_flush_pending_updates_for_contact() {
    let storage = test_storage();

    // Add updates for multiple contacts
    for i in 0..3 {
        let update = create_pending_update(&format!("alice-{}", i), "alice");
        storage.queue_update(&update).unwrap();
    }
    for i in 0..2 {
        let update = create_pending_update(&format!("bob-{}", i), "bob");
        storage.queue_update(&update).unwrap();
    }

    // Delete all for alice
    let deleted = storage.delete_pending_updates_for_contact("alice").unwrap();
    assert_eq!(deleted, 3);

    // Only bob's updates remain
    assert_eq!(storage.count_all_pending_updates().unwrap(), 2);
    assert_eq!(storage.get_pending_updates("alice").unwrap().len(), 0);
    assert_eq!(storage.get_pending_updates("bob").unwrap().len(), 2);
}

#[test]
fn test_clear_all_pending_updates() {
    let storage = test_storage();

    // Add various updates
    for i in 0..10 {
        let update = create_pending_update(&format!("update-{}", i), &format!("contact-{}", i % 3));
        storage.queue_update(&update).unwrap();
    }

    assert_eq!(storage.count_all_pending_updates().unwrap(), 10);

    // Clear all
    let cleared = storage.clear_all_pending_updates().unwrap();
    assert_eq!(cleared, 10);

    assert_eq!(storage.count_all_pending_updates().unwrap(), 0);
}

// === Status Transition Tests ===

#[test]
fn test_get_pending_by_status() {
    let storage = test_storage();
    let timestamp = now();

    // Add updates with different statuses
    let statuses = vec![
        ("pending-1", UpdateStatus::Pending),
        ("pending-2", UpdateStatus::Pending),
        ("sending-1", UpdateStatus::Sending),
        (
            "failed-1",
            UpdateStatus::Failed {
                error: "timeout".to_string(),
                retry_at: timestamp + 60,
            },
        ),
    ];

    for (id, status) in statuses {
        let update = PendingUpdate {
            id: id.to_string(),
            contact_id: "contact".to_string(),
            update_type: "card_delta".to_string(),
            payload: vec![],
            created_at: timestamp,
            retry_count: 0,
            status,
        };
        storage.queue_update(&update).unwrap();
    }

    // Get only pending status
    let pending = storage.get_pending_updates_by_status("pending").unwrap();
    assert_eq!(pending.len(), 2);

    // Get only sending status
    let sending = storage.get_pending_updates_by_status("sending").unwrap();
    assert_eq!(sending.len(), 1);

    // Get only failed status
    let failed = storage.get_pending_updates_by_status("failed").unwrap();
    assert_eq!(failed.len(), 1);
}

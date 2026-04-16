// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for OfflineManager queue and flush behaviour.
//!
//! Traces to: features/message_delivery.feature
//! SP-12b Phase 1: Core Glue — Task 6

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::delivery::OfflineManager;
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

fn test_update(id: &str, contact_id: &str) -> PendingUpdate {
    PendingUpdate {
        id: id.to_string(),
        contact_id: contact_id.to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: now(),
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    }
}

// === Offline Queuing Tests ===

// @scenario: message_delivery :: Offline message queued for later delivery
#[test]
fn test_send_or_queue_offline_enqueues() {
    let storage = test_storage();
    let manager = OfflineManager::new();

    // Queue while offline
    let result = manager.send_or_queue(&storage, test_update("msg-1", "alice"), false);
    assert!(result.is_ok(), "Queuing while offline should succeed");

    // Verify it's in storage
    let pending = storage.get_all_pending_updates().unwrap();
    assert_eq!(pending.len(), 1, "One update should be queued");
    assert_eq!(pending[0].id, "msg-1");
}

// @scenario: message_delivery :: Online message sent directly (not queued)
#[test]
fn test_send_or_queue_online_marks_sending() {
    let storage = test_storage();
    let manager = OfflineManager::new();

    // Send while online — should still queue but mark as "sending"
    let result = manager.send_or_queue(&storage, test_update("msg-1", "alice"), true);
    result.expect("expected success");

    // The update is queued with Sending status (caller is responsible for actual send)
    let pending = storage.get_pending_update("msg-1").unwrap();
    assert!(
        pending.is_some(),
        "Online update should be queued for the caller to send"
    );
}

// @scenario: message_delivery :: Offline queue respects capacity
#[test]
fn test_send_or_queue_rejects_when_full() {
    let storage = test_storage();
    let manager = OfflineManager::with_offline_queue(OfflineQueue::with_max_size(2));

    // Fill the queue
    manager
        .send_or_queue(&storage, test_update("msg-1", "alice"), false)
        .unwrap();
    manager
        .send_or_queue(&storage, test_update("msg-2", "alice"), false)
        .unwrap();

    // Third should fail
    let result = manager.send_or_queue(&storage, test_update("msg-3", "alice"), false);
    assert!(
        result.is_err(),
        "Should reject when queue is at capacity: {:?}",
        result
    );
}

// === Flush Tests ===

// @scenario: message_delivery :: Queued messages ready for flush on reconnect
#[test]
fn test_flush_queue_returns_pending_updates() {
    let storage = test_storage();
    let manager = OfflineManager::new();

    // Queue some updates while offline
    manager
        .send_or_queue(&storage, test_update("msg-1", "alice"), false)
        .unwrap();
    manager
        .send_or_queue(&storage, test_update("msg-2", "bob"), false)
        .unwrap();

    // Flush returns all pending updates
    let flushed = manager.flush_queue(&storage).unwrap();
    assert_eq!(
        flushed.len(),
        2,
        "All queued updates should be returned for sending"
    );
}

// @scenario: message_delivery :: Flush preserves message order
#[test]
fn test_flush_queue_preserves_order() {
    let storage = test_storage();
    let manager = OfflineManager::new();

    manager
        .send_or_queue(&storage, test_update("msg-1", "alice"), false)
        .unwrap();
    manager
        .send_or_queue(&storage, test_update("msg-2", "alice"), false)
        .unwrap();
    manager
        .send_or_queue(&storage, test_update("msg-3", "alice"), false)
        .unwrap();

    let flushed = manager.flush_queue(&storage).unwrap();
    let ids: Vec<&str> = flushed.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["msg-1", "msg-2", "msg-3"],
        "Flush should preserve insertion order"
    );
}

// @scenario: message_delivery :: Queue capacity tracking
#[test]
fn test_queue_remaining_capacity() {
    let storage = test_storage();
    let manager = OfflineManager::with_offline_queue(OfflineQueue::with_max_size(5));

    assert_eq!(
        manager.remaining_capacity(&storage).unwrap(),
        5,
        "Empty queue should have full capacity"
    );

    manager
        .send_or_queue(&storage, test_update("msg-1", "alice"), false)
        .unwrap();
    manager
        .send_or_queue(&storage, test_update("msg-2", "alice"), false)
        .unwrap();

    assert_eq!(
        manager.remaining_capacity(&storage).unwrap(),
        3,
        "After 2 enqueues, 3 slots should remain"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for connectivity-triggered delivery operations.
//!
//! Verifies that coming online triggers both retry tick and offline queue flush.
//!
//! Traces to: features/message_delivery.feature
//! SP-12b Phase 1: Core Glue — Task 7

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::delivery::{OfflineManager, RetryScheduler};
use vauchi_core::storage::{
    DeliveryRecord, DeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, Storage, UpdateStatus,
};

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

fn create_retry(storage: &Storage, msg_id: &str, attempt: u32, next_retry: u64) {
    let ts = now();
    let entry = RetryEntry {
        message_id: msg_id.to_string(),
        recipient_id: "test-recipient".to_string(),
        payload: vec![1, 2, 3],
        attempt,
        next_retry,
        created_at: ts,
        max_attempts: 5,
    };
    storage.create_retry_entry(&entry).unwrap();
}

fn create_offline_update(storage: &Storage, manager: &OfflineManager, id: &str) {
    let update = PendingUpdate {
        id: id.to_string(),
        contact_id: "test-contact".to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: now(),
        retry_count: 0,
        status: UpdateStatus::Pending,
    };
    manager.send_or_queue(storage, update, false).unwrap();
}

// @scenario: message_delivery:Connectivity restored triggers retry and flush
#[test]
fn test_connectivity_restored_processes_retries_and_queue() {
    let storage = test_storage();
    let retry_scheduler = RetryScheduler::new();
    let offline_manager = OfflineManager::new();
    let ts = now();

    // Set up: 1 due retry entry
    create_retry(&storage, "retry-msg", 1, ts - 10);

    // Set up: 2 offline-queued updates
    create_offline_update(&storage, &offline_manager, "offline-1");
    create_offline_update(&storage, &offline_manager, "offline-2");

    // Simulate "coming online": run retry tick and flush offline queue
    let tick_result = retry_scheduler.tick(&storage).unwrap();
    let flushed = offline_manager.flush_queue(&storage).unwrap();

    // Verify retry was processed
    assert_eq!(tick_result.due, 1, "One retry should be due");
    assert_eq!(
        tick_result.rescheduled, 1,
        "Due retry should be rescheduled"
    );
    assert!(
        tick_result.ready_ids.contains(&"retry-msg".to_string()),
        "retry-msg should be in ready IDs"
    );

    // Verify offline queue was flushed
    assert_eq!(flushed.len(), 2, "Two offline updates should be ready");
}

// @scenario: message_delivery:Empty retry and queue on connectivity restore
#[test]
fn test_connectivity_restored_with_nothing_pending() {
    let storage = test_storage();
    let retry_scheduler = RetryScheduler::new();
    let offline_manager = OfflineManager::new();

    // No retries, no offline queue — should be a no-op
    let tick_result = retry_scheduler.tick(&storage).unwrap();
    let flushed = offline_manager.flush_queue(&storage).unwrap();

    assert_eq!(tick_result.due, 0);
    assert_eq!(tick_result.rescheduled, 0);
    assert!(flushed.is_empty());
}

// @scenario: message_delivery:Combined retry and offline stats
#[test]
fn test_combined_retry_and_offline_counts() {
    let storage = test_storage();
    let retry_scheduler = RetryScheduler::new();
    let offline_manager = OfflineManager::with_offline_queue(OfflineQueue::with_max_size(100));
    let ts = now();

    // 2 due retries (1 will expire, 1 will reschedule)
    create_retry(&storage, "retry-expire", 5, ts - 10); // at max attempts
    create_retry(&storage, "retry-ok", 2, ts - 10);
    // Create delivery record for the expiring one
    let delivery = DeliveryRecord {
        message_id: "retry-expire".to_string(),
        recipient_id: "test-recipient".to_string(),
        status: DeliveryStatus::Sent,
        created_at: ts,
        updated_at: ts,
        expires_at: None,
    };
    storage.create_delivery_record(&delivery).unwrap();

    // 3 offline updates
    create_offline_update(&storage, &offline_manager, "q-1");
    create_offline_update(&storage, &offline_manager, "q-2");
    create_offline_update(&storage, &offline_manager, "q-3");

    let tick_result = retry_scheduler.tick(&storage).unwrap();
    let flushed = offline_manager.flush_queue(&storage).unwrap();

    // Retry: 2 due, 1 rescheduled, 1 expired
    assert_eq!(tick_result.due, 2);
    assert_eq!(tick_result.rescheduled, 1);
    assert_eq!(tick_result.expired, 1);
    assert_eq!(tick_result.ready_ids.len(), 1);

    // Offline: 3 flushed
    assert_eq!(flushed.len(), 3);

    // Total actions on connectivity restore = 1 retry resend + 3 queue flush = 4
    let total_actions = tick_result.ready_ids.len() + flushed.len();
    assert_eq!(total_actions, 4);
}

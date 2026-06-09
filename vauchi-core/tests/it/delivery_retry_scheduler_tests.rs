// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for RetryScheduler tick processing.
//!
//! Traces to: features/message_delivery.feature
//! SP-12b Phase 1: Core Glue — Task 5

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::delivery::RetryScheduler;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus, RetryEntry, Storage};

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
    storage.retries().create_retry_entry(&entry).unwrap();
}

fn create_delivery(storage: &Storage, msg_id: &str) {
    let ts = now();
    let record = DeliveryRecord {
        message_id: msg_id.to_string(),
        recipient_id: "test-recipient".to_string(),
        status: DeliveryStatus::Sent,
        created_at: ts,
        updated_at: ts,
        expires_at: None,
    };
    storage
        .deliveries()
        .create_delivery_record(&record)
        .unwrap();
}

// === Tick Processing Tests ===

// @scenario: message_delivery :: Automatic retry on transient failure
#[test]
fn test_tick_processes_due_retries_only() {
    let storage = test_storage();
    let scheduler = RetryScheduler::new();
    let ts = now();

    // 2 due (past), 1 not due (future)
    create_retry(&storage, "due-1", 0, ts - 10);
    create_retry(&storage, "due-2", 1, ts - 5);
    create_retry(&storage, "future", 0, ts + 1000);

    let result = scheduler
        .tick(&storage, &vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(result.due, 2, "Only 2 entries should be due");
    assert_eq!(
        result.rescheduled, 2,
        "Both due entries should be rescheduled"
    );
    assert_eq!(result.expired, 0, "No entries should be expired");

    let future = storage
        .retries()
        .get_retry_entry("future")
        .unwrap()
        .unwrap();
    assert_eq!(future.attempt, 0, "Future entry should not be touched");
}

// @scenario: message_delivery :: Exponential backoff with jitter
#[test]
fn test_tick_increments_attempt_count() {
    let storage = test_storage();
    let scheduler = RetryScheduler::new();
    let ts = now();

    create_retry(&storage, "retry-inc", 2, ts - 10);

    scheduler
        .tick(&storage, &vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    let entry = storage
        .retries()
        .get_retry_entry("retry-inc")
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.attempt, 3,
        "Attempt count should be incremented from 2 to 3"
    );
    assert!(
        entry.next_retry > ts,
        "Next retry should be scheduled in the future"
    );
}

// @scenario: message_delivery :: Give up after maximum retries
#[test]
fn test_tick_removes_max_attempt_entries() {
    let storage = test_storage();
    let scheduler = RetryScheduler::new();
    let ts = now();

    // Entry at max attempts (attempt=5, max_attempts=5)
    create_retry(&storage, "max-out", 5, ts - 10);
    create_delivery(&storage, "max-out");

    // Entry under max (should be rescheduled)
    create_retry(&storage, "still-ok", 2, ts - 10);

    let result = scheduler
        .tick(&storage, &vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(result.expired, 1, "One entry should be expired");
    assert_eq!(result.rescheduled, 1, "One entry should be rescheduled");

    assert!(
        storage
            .retries()
            .get_retry_entry("max-out")
            .unwrap()
            .is_none(),
        "Max-attempt retry entry should be deleted"
    );

    let delivery = storage
        .deliveries()
        .get_delivery_record("max-out")
        .unwrap()
        .unwrap();
    assert!(
        matches!(delivery.status, DeliveryStatus::Failed { .. }),
        "Delivery status should be Failed after max retries, got: {:?}",
        delivery.status
    );

    let still_ok = storage
        .retries()
        .get_retry_entry("still-ok")
        .unwrap()
        .unwrap();
    assert_eq!(still_ok.attempt, 3);
}

// @scenario: message_delivery :: Retry tick returns ready IDs for resend
#[test]
fn test_tick_returns_ready_message_ids() {
    let storage = test_storage();
    let scheduler = RetryScheduler::new();
    let ts = now();

    create_retry(&storage, "ready-1", 0, ts - 10);
    create_retry(&storage, "ready-2", 1, ts - 5);
    create_retry(&storage, "maxed", 5, ts - 1); // At max, should NOT be in ready_ids

    let result = scheduler
        .tick(&storage, &vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(result.ready_ids.len(), 2, "Two entries should be ready");
    assert!(result.ready_ids.contains(&"ready-1".to_string()));
    assert!(result.ready_ids.contains(&"ready-2".to_string()));
    assert!(
        !result.ready_ids.contains(&"maxed".to_string()),
        "Max-attempt entries should not be in ready_ids"
    );
}

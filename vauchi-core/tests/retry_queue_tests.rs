//! Tests for retry queue operations.
//!
//! Traces to: features/message_delivery.feature
//! P14 Phase 4: Retry Queue

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{RetryEntry, RetryQueue, Storage};

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

// === Exponential Backoff Tests ===

#[test]
fn test_exponential_backoff_calculation() {
    // Backoff: 1s, 2s, 4s, 8s, 16s, 32s, 64s, 128s, 256s, 512s... max 3600s (1h)
    let queue = RetryQueue::new();

    assert_eq!(queue.backoff_seconds(0), 1);    // 2^0 = 1
    assert_eq!(queue.backoff_seconds(1), 2);    // 2^1 = 2
    assert_eq!(queue.backoff_seconds(2), 4);    // 2^2 = 4
    assert_eq!(queue.backoff_seconds(3), 8);    // 2^3 = 8
    assert_eq!(queue.backoff_seconds(10), 1024); // 2^10 = 1024
    assert_eq!(queue.backoff_seconds(12), 3600); // 2^12 = 4096 > 3600, capped at 3600
    assert_eq!(queue.backoff_seconds(20), 3600); // Always capped at 1 hour
}

#[test]
fn test_calculate_next_retry_time() {
    let queue = RetryQueue::new();
    let base_time = 1000u64;

    // First retry: base_time + 1s
    assert_eq!(queue.next_retry_time(base_time, 0), 1001);

    // Second retry: base_time + 2s
    assert_eq!(queue.next_retry_time(base_time, 1), 1002);

    // Third retry: base_time + 4s
    assert_eq!(queue.next_retry_time(base_time, 2), 1004);
}

// === Retry Entry Storage Tests ===

#[test]
fn test_create_and_retrieve_retry_entry() {
    let storage = test_storage();
    let timestamp = now();

    let entry = RetryEntry {
        message_id: "retry-msg-001".to_string(),
        recipient_id: "contact-abc".to_string(),
        payload: vec![1, 2, 3, 4, 5],
        attempt: 0,
        next_retry: timestamp + 1,
        created_at: timestamp,
        max_attempts: 10,
    };

    storage.create_retry_entry(&entry).unwrap();

    let retrieved = storage.get_retry_entry("retry-msg-001").unwrap();
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.message_id, "retry-msg-001");
    assert_eq!(retrieved.recipient_id, "contact-abc");
    assert_eq!(retrieved.payload, vec![1, 2, 3, 4, 5]);
    assert_eq!(retrieved.attempt, 0);
    assert_eq!(retrieved.max_attempts, 10);
}

#[test]
fn test_get_due_retries() {
    let storage = test_storage();
    let now_ts = now();

    // Create entries: 2 due, 1 not due
    let entries = vec![
        ("msg-1", now_ts - 10), // Due (past)
        ("msg-2", now_ts - 5),  // Due (past)
        ("msg-3", now_ts + 100), // Not due (future)
    ];

    for (id, next_retry) in entries {
        let entry = RetryEntry {
            message_id: id.to_string(),
            recipient_id: "contact".to_string(),
            payload: vec![],
            attempt: 0,
            next_retry,
            created_at: now_ts,
            max_attempts: 10,
        };
        storage.create_retry_entry(&entry).unwrap();
    }

    let due = storage.get_due_retries(now_ts).unwrap();
    assert_eq!(due.len(), 2);

    let due_ids: Vec<_> = due.iter().map(|e| e.message_id.as_str()).collect();
    assert!(due_ids.contains(&"msg-1"));
    assert!(due_ids.contains(&"msg-2"));
}

#[test]
fn test_increment_retry_attempt() {
    let storage = test_storage();
    let timestamp = now();

    let entry = RetryEntry {
        message_id: "retry-inc".to_string(),
        recipient_id: "contact".to_string(),
        payload: vec![],
        attempt: 0,
        next_retry: timestamp + 1,
        created_at: timestamp,
        max_attempts: 10,
    };
    storage.create_retry_entry(&entry).unwrap();

    // Increment attempt
    let new_next_retry = timestamp + 100;
    storage
        .increment_retry_attempt("retry-inc", new_next_retry)
        .unwrap();

    let retrieved = storage.get_retry_entry("retry-inc").unwrap().unwrap();
    assert_eq!(retrieved.attempt, 1);
    assert_eq!(retrieved.next_retry, new_next_retry);
}

#[test]
fn test_delete_retry_entry() {
    let storage = test_storage();
    let timestamp = now();

    let entry = RetryEntry {
        message_id: "to-delete".to_string(),
        recipient_id: "contact".to_string(),
        payload: vec![],
        attempt: 0,
        next_retry: timestamp + 1,
        created_at: timestamp,
        max_attempts: 10,
    };
    storage.create_retry_entry(&entry).unwrap();

    assert!(storage.get_retry_entry("to-delete").unwrap().is_some());

    let deleted = storage.delete_retry_entry("to-delete").unwrap();
    assert!(deleted);

    assert!(storage.get_retry_entry("to-delete").unwrap().is_none());
}

#[test]
fn test_max_attempts_exceeded() {
    let storage = test_storage();
    let timestamp = now();

    let entry = RetryEntry {
        message_id: "max-retry".to_string(),
        recipient_id: "contact".to_string(),
        payload: vec![],
        attempt: 9, // One more attempt allowed (max is 10)
        next_retry: timestamp,
        created_at: timestamp,
        max_attempts: 10,
    };
    storage.create_retry_entry(&entry).unwrap();

    let retrieved = storage.get_retry_entry("max-retry").unwrap().unwrap();
    assert!(!retrieved.is_max_attempts_exceeded()); // 9 < 10

    // Increment to 10
    storage
        .increment_retry_attempt("max-retry", timestamp + 100)
        .unwrap();

    let retrieved = storage.get_retry_entry("max-retry").unwrap().unwrap();
    assert!(retrieved.is_max_attempts_exceeded()); // 10 >= 10
}

#[test]
fn test_get_all_retry_entries() {
    let storage = test_storage();
    let timestamp = now();

    for i in 0..5 {
        let entry = RetryEntry {
            message_id: format!("msg-{}", i),
            recipient_id: "contact".to_string(),
            payload: vec![],
            attempt: i,
            next_retry: timestamp + i as u64,
            created_at: timestamp,
            max_attempts: 10,
        };
        storage.create_retry_entry(&entry).unwrap();
    }

    let all = storage.get_all_retry_entries().unwrap();
    assert_eq!(all.len(), 5);
}

#[test]
fn test_count_retry_entries() {
    let storage = test_storage();
    let timestamp = now();

    assert_eq!(storage.count_retry_entries().unwrap(), 0);

    for i in 0..3 {
        let entry = RetryEntry {
            message_id: format!("count-{}", i),
            recipient_id: "contact".to_string(),
            payload: vec![],
            attempt: 0,
            next_retry: timestamp,
            created_at: timestamp,
            max_attempts: 10,
        };
        storage.create_retry_entry(&entry).unwrap();
    }

    assert_eq!(storage.count_retry_entries().unwrap(), 3);
}

#[test]
fn test_retry_entry_for_recipient() {
    let storage = test_storage();
    let timestamp = now();

    // Create entries for two recipients
    for i in 0..3 {
        let entry = RetryEntry {
            message_id: format!("alice-{}", i),
            recipient_id: "alice".to_string(),
            payload: vec![],
            attempt: 0,
            next_retry: timestamp,
            created_at: timestamp,
            max_attempts: 10,
        };
        storage.create_retry_entry(&entry).unwrap();
    }

    for i in 0..2 {
        let entry = RetryEntry {
            message_id: format!("bob-{}", i),
            recipient_id: "bob".to_string(),
            payload: vec![],
            attempt: 0,
            next_retry: timestamp,
            created_at: timestamp,
            max_attempts: 10,
        };
        storage.create_retry_entry(&entry).unwrap();
    }

    let alice_entries = storage.get_retry_entries_for_recipient("alice").unwrap();
    assert_eq!(alice_entries.len(), 3);

    let bob_entries = storage.get_retry_entries_for_recipient("bob").unwrap();
    assert_eq!(bob_entries.len(), 2);
}

// === Integration Tests ===

#[test]
fn test_retry_lifecycle() {
    let storage = test_storage();
    let queue = RetryQueue::new();
    let timestamp = now();

    // 1. Create initial retry entry (attempt=0)
    let entry = RetryEntry {
        message_id: "lifecycle".to_string(),
        recipient_id: "contact".to_string(),
        payload: b"test payload".to_vec(),
        attempt: 0,
        next_retry: queue.next_retry_time(timestamp, 0),
        created_at: timestamp,
        max_attempts: 3,
    };
    storage.create_retry_entry(&entry).unwrap();

    // 2. Simulate retry attempts (need 3 increments to reach max_attempts=3)
    for expected_attempt in 1..=3 {
        let next_retry = queue.next_retry_time(timestamp, expected_attempt);
        storage
            .increment_retry_attempt("lifecycle", next_retry)
            .unwrap();

        let entry = storage.get_retry_entry("lifecycle").unwrap().unwrap();
        assert_eq!(entry.attempt, expected_attempt);
    }

    // 3. After max attempts (attempt=3, max=3), entry should be marked for removal
    let entry = storage.get_retry_entry("lifecycle").unwrap().unwrap();
    assert!(entry.is_max_attempts_exceeded()); // 3 >= 3 is true

    // 4. Delete after max attempts
    storage.delete_retry_entry("lifecycle").unwrap();
    assert!(storage.get_retry_entry("lifecycle").unwrap().is_none());
}

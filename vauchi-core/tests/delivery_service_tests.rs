// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for DeliveryService ACK handling.
//!
//! Traces to: features/message_delivery.feature
//! SP-12b Phase 1: Core Glue — Task 3

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::delivery::{DeliveryAckStatus, DeliveryService};
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

fn create_test_delivery(storage: &Storage, message_id: &str, status: DeliveryStatus) {
    let ts = now();
    let record = DeliveryRecord {
        message_id: message_id.to_string(),
        recipient_id: "test-recipient".to_string(),
        status,
        created_at: ts,
        updated_at: ts,
        expires_at: None,
    };
    storage.create_delivery_record(&record).unwrap();
}

// === ACK Handling Tests ===

// @scenario: message_delivery:Relay provides storage confirmation
#[test]
fn test_handle_stored_ack_updates_delivery_record() {
    let storage = test_storage();
    let service = DeliveryService::new();

    create_test_delivery(&storage, "msg-1", DeliveryStatus::Queued);

    service
        .handle_ack(&storage, "msg-1", DeliveryAckStatus::Stored)
        .unwrap();

    let record = storage.get_delivery_record("msg-1").unwrap().unwrap();
    assert_eq!(
        record.status,
        DeliveryStatus::Stored,
        "Stored ACK should update delivery record status to Stored"
    );
}

// @scenario: message_delivery:Delivered ACK removes retry entry
#[test]
fn test_handle_delivered_ack_removes_retry_entry() {
    let storage = test_storage();
    let service = DeliveryService::new();

    create_test_delivery(&storage, "msg-1", DeliveryStatus::Stored);

    // Pre-existing retry entry should be cleaned up on delivery
    let ts = now();
    let retry = RetryEntry {
        message_id: "msg-1".to_string(),
        recipient_id: "test-recipient".to_string(),
        payload: vec![1, 2, 3],
        attempt: 1,
        next_retry: ts + 60,
        created_at: ts,
        max_attempts: 10,
    };
    storage.create_retry_entry(&retry).unwrap();

    service
        .handle_ack(&storage, "msg-1", DeliveryAckStatus::Delivered)
        .unwrap();

    let record = storage.get_delivery_record("msg-1").unwrap().unwrap();
    assert_eq!(
        record.status,
        DeliveryStatus::Delivered,
        "Delivered ACK should update delivery record status to Delivered"
    );
    assert!(
        storage.get_retry_entry("msg-1").unwrap().is_none(),
        "Retry entry should be removed on successful delivery"
    );
}

// @scenario: message_delivery:Failed ACK schedules retry with backoff
#[test]
fn test_handle_failed_ack_schedules_retry() {
    let storage = test_storage();
    let service = DeliveryService::new();

    create_test_delivery(&storage, "msg-1", DeliveryStatus::Sent);

    service
        .handle_ack(
            &storage,
            "msg-1",
            DeliveryAckStatus::Failed {
                reason: "timeout".to_string(),
            },
        )
        .unwrap();

    let record = storage.get_delivery_record("msg-1").unwrap().unwrap();
    assert_eq!(
        record.status,
        DeliveryStatus::Failed {
            reason: "timeout".to_string()
        },
        "Failed ACK should update delivery record status to Failed with reason"
    );

    let retry = storage.get_retry_entry("msg-1").unwrap();
    assert!(
        retry.is_some(),
        "Failed delivery should schedule a retry entry"
    );

    let retry = retry.unwrap();
    assert_eq!(retry.message_id, "msg-1");
    assert_eq!(retry.recipient_id, "test-recipient");
    assert_eq!(retry.attempt, 0, "First retry should have attempt=0");
    assert!(
        retry.next_retry > now().saturating_sub(5),
        "Retry should be scheduled in the future"
    );
}

// === AckStatus Conversion Tests ===

// @scenario: message_delivery:Network ACK status converts to delivery ACK status
#[test]
fn test_ack_status_conversion_from_network() {
    use vauchi_core::network::AckStatus;

    // Stored → Stored
    let stored = DeliveryAckStatus::from_network_ack(AckStatus::Stored, None);
    assert_eq!(stored, DeliveryAckStatus::Stored);

    // Delivered → Delivered
    let delivered = DeliveryAckStatus::from_network_ack(AckStatus::Delivered, None);
    assert_eq!(delivered, DeliveryAckStatus::Delivered);

    // ReceivedByRecipient → Delivered
    let received = DeliveryAckStatus::from_network_ack(AckStatus::ReceivedByRecipient, None);
    assert_eq!(received, DeliveryAckStatus::Delivered);

    // Failed → Failed with reason
    let failed = DeliveryAckStatus::from_network_ack(AckStatus::Failed, Some("timeout"));
    assert_eq!(
        failed,
        DeliveryAckStatus::Failed {
            reason: "timeout".to_string()
        }
    );

    // Failed without error message → Failed with "unknown"
    let failed_no_reason = DeliveryAckStatus::from_network_ack(AckStatus::Failed, None);
    assert_eq!(
        failed_no_reason,
        DeliveryAckStatus::Failed {
            reason: "unknown".to_string()
        }
    );
}

// @scenario: message_delivery:Unknown message ACK handled gracefully
#[test]
fn test_handle_ack_for_unknown_message_returns_error() {
    let storage = test_storage();
    let service = DeliveryService::new();

    let result = service.handle_ack(&storage, "nonexistent", DeliveryAckStatus::Delivered);
    assert!(
        result.is_err(),
        "ACK for unknown message should return NotFound error"
    );

    // Verify it's specifically a NotFound error
    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Not found"),
        "Error should be NotFound variant, got: {}",
        err_msg
    );
}

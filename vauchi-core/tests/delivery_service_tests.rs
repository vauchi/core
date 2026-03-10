// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for DeliveryService ACK handling.
//!
//! Traces to: features/message_delivery.feature
//! SP-12b Phase 1: Core Glue — Task 3

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::delivery::{DeliveryAckStatus, DeliveryService};
use vauchi_core::storage::{
    DeliveryRecord, DeliveryStatus, DeviceDeliveryRecord, DeviceDeliveryStatus, RetryEntry, Storage,
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

// === Expiration Cleanup Tests (Task 8) ===

fn create_delivery_with_expiry(
    storage: &Storage,
    message_id: &str,
    status: DeliveryStatus,
    created_at: u64,
    expires_at: Option<u64>,
) {
    let record = DeliveryRecord {
        message_id: message_id.to_string(),
        recipient_id: "test-recipient".to_string(),
        status,
        created_at,
        updated_at: created_at,
        expires_at,
    };
    storage.create_delivery_record(&record).unwrap();
}

// @scenario: message_delivery:Expired messages are cleaned up automatically
#[test]
fn test_run_cleanup_expires_old_records() {
    let storage = test_storage();
    let service = DeliveryService::new();
    let ts = now();

    // Record with expiry in the past — should be expired
    create_delivery_with_expiry(
        &storage,
        "msg-expired",
        DeliveryStatus::Sent,
        ts - 3600,
        Some(ts - 60),
    );

    // Record with expiry in the future — should survive
    create_delivery_with_expiry(
        &storage,
        "msg-valid",
        DeliveryStatus::Sent,
        ts - 100,
        Some(ts + 3600),
    );

    // Record with no expiry — should survive
    create_delivery_with_expiry(
        &storage,
        "msg-no-expiry",
        DeliveryStatus::Stored,
        ts - 100,
        None,
    );

    let result = service.run_cleanup(&storage).unwrap();
    assert_eq!(result.expired, 1, "One record should be expired");
    assert_eq!(result.cleaned_up, 0, "No records old enough for cleanup");

    // Verify the expired record was marked
    let expired = storage.get_delivery_record("msg-expired").unwrap().unwrap();
    assert_eq!(
        expired.status,
        DeliveryStatus::Expired,
        "Past-expiry record should be marked Expired"
    );

    // Verify others are untouched
    let valid = storage.get_delivery_record("msg-valid").unwrap().unwrap();
    assert_eq!(valid.status, DeliveryStatus::Sent);
    let no_exp = storage
        .get_delivery_record("msg-no-expiry")
        .unwrap()
        .unwrap();
    assert_eq!(no_exp.status, DeliveryStatus::Stored);
}

// @scenario: message_delivery:Old terminal records are cleaned up
#[test]
fn test_run_cleanup_removes_old_terminal_records() {
    let storage = test_storage();
    let service = DeliveryService::new();
    let ts = now();
    let thirty_one_days_ago = ts - (31 * 24 * 3600);

    // Old delivered record — should be cleaned up
    create_delivery_with_expiry(
        &storage,
        "msg-old-delivered",
        DeliveryStatus::Delivered,
        thirty_one_days_ago,
        None,
    );
    // Update its updated_at to be old
    storage
        .update_delivery_status(
            "msg-old-delivered",
            &DeliveryStatus::Delivered,
            thirty_one_days_ago,
        )
        .unwrap();

    // Recent delivered record — should survive
    create_delivery_with_expiry(
        &storage,
        "msg-recent-delivered",
        DeliveryStatus::Delivered,
        ts - 3600,
        None,
    );

    let result = service.run_cleanup(&storage).unwrap();
    assert_eq!(
        result.cleaned_up, 1,
        "One old terminal record should be cleaned up"
    );

    // Old record should be removed
    assert!(
        storage
            .get_delivery_record("msg-old-delivered")
            .unwrap()
            .is_none(),
        "Old delivered record should be removed"
    );

    // Recent record should survive
    assert!(
        storage
            .get_delivery_record("msg-recent-delivered")
            .unwrap()
            .is_some(),
        "Recent delivered record should survive"
    );
}

// === Per-Device ACK Tracking Tests (SP-12b Task 21) ===

fn create_device_record(storage: &Storage, message_id: &str, device_id: &str, recipient_id: &str) {
    let record = DeviceDeliveryRecord {
        message_id: message_id.to_string(),
        device_id: device_id.to_string(),
        recipient_id: recipient_id.to_string(),
        status: DeviceDeliveryStatus::Pending,
        updated_at: now(),
    };
    storage.create_device_delivery(&record).unwrap();
}

// @scenario: message_delivery:Per-device ACK tracking aggregates to full delivery
#[test]
fn test_handle_device_ack_tracks_per_device_delivery() {
    let storage = test_storage();
    let service = DeliveryService::new();

    // Create message-level delivery record
    create_test_delivery(&storage, "msg-multi", DeliveryStatus::Stored);

    // Register 3 target devices
    create_device_record(&storage, "msg-multi", "device-a", "recipient-1");
    create_device_record(&storage, "msg-multi", "device-b", "recipient-1");
    create_device_record(&storage, "msg-multi", "device-c", "recipient-1");

    // ACK device-a → not fully delivered
    let summary = service
        .handle_device_ack(
            &storage,
            "msg-multi",
            "device-a",
            DeviceDeliveryStatus::Delivered,
        )
        .unwrap();
    assert_eq!(summary.delivered_devices, 1);
    assert_eq!(summary.total_devices, 3);
    assert!(
        !summary.is_fully_delivered(),
        "1/3 devices — not fully delivered"
    );

    // Message-level status should still be Stored (not yet fully delivered)
    let record = storage.get_delivery_record("msg-multi").unwrap().unwrap();
    assert_eq!(record.status, DeliveryStatus::Stored);

    // ACK device-b → still not fully delivered
    let summary = service
        .handle_device_ack(
            &storage,
            "msg-multi",
            "device-b",
            DeviceDeliveryStatus::Delivered,
        )
        .unwrap();
    assert_eq!(summary.delivered_devices, 2);
    assert!(
        !summary.is_fully_delivered(),
        "2/3 devices — not fully delivered"
    );

    // ACK device-c → fully delivered
    let summary = service
        .handle_device_ack(
            &storage,
            "msg-multi",
            "device-c",
            DeviceDeliveryStatus::Delivered,
        )
        .unwrap();
    assert_eq!(summary.delivered_devices, 3);
    assert!(
        summary.is_fully_delivered(),
        "3/3 devices — fully delivered"
    );

    // Message-level status should now be Delivered
    let record = storage.get_delivery_record("msg-multi").unwrap().unwrap();
    assert_eq!(
        record.status,
        DeliveryStatus::Delivered,
        "Message-level status should be Delivered when all devices confirmed"
    );
}

// @scenario: message_delivery:Device ACK for unknown message returns error
#[test]
fn test_handle_device_ack_unknown_message_returns_error() {
    let storage = test_storage();
    let service = DeliveryService::new();

    let result = service.handle_device_ack(
        &storage,
        "nonexistent",
        "device-a",
        DeviceDeliveryStatus::Delivered,
    );
    assert!(
        result.is_err(),
        "Device ACK for unknown message should return error"
    );
}

// @scenario: message_delivery:Failed device ACK does not mark message delivered
#[test]
fn test_handle_device_ack_failed_device_not_fully_delivered() {
    let storage = test_storage();
    let service = DeliveryService::new();

    create_test_delivery(&storage, "msg-fail", DeliveryStatus::Stored);
    create_device_record(&storage, "msg-fail", "device-a", "recipient-1");
    create_device_record(&storage, "msg-fail", "device-b", "recipient-1");

    // device-a delivered, device-b failed
    service
        .handle_device_ack(
            &storage,
            "msg-fail",
            "device-a",
            DeviceDeliveryStatus::Delivered,
        )
        .unwrap();
    let summary = service
        .handle_device_ack(
            &storage,
            "msg-fail",
            "device-b",
            DeviceDeliveryStatus::Failed,
        )
        .unwrap();

    assert_eq!(summary.delivered_devices, 1);
    assert_eq!(summary.failed_devices, 1);
    assert!(!summary.is_fully_delivered());

    // Message-level should NOT be Delivered
    let record = storage.get_delivery_record("msg-fail").unwrap().unwrap();
    assert_ne!(
        record.status,
        DeliveryStatus::Delivered,
        "Message should not be marked Delivered when a device failed"
    );
}

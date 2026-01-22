//! Tests for multi-device delivery tracking.
//!
//! Traces to: features/message_delivery.feature
//! P14 Phase 6: Multi-Device Delivery

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{DeviceDeliveryRecord, DeviceDeliveryStatus, Storage};

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

// === Device Delivery Record Tests ===

#[test]
fn test_create_device_delivery_record() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeviceDeliveryRecord {
        message_id: "msg-001".to_string(),
        recipient_id: "contact-abc".to_string(),
        device_id: "device-1".to_string(),
        status: DeviceDeliveryStatus::Pending,
        updated_at: timestamp,
    };

    storage.create_device_delivery(&record).unwrap();

    let retrieved = storage
        .get_device_delivery("msg-001", "device-1")
        .unwrap();
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.message_id, "msg-001");
    assert_eq!(retrieved.device_id, "device-1");
    assert_eq!(retrieved.status, DeviceDeliveryStatus::Pending);
}

#[test]
fn test_track_multiple_devices_for_message() {
    let storage = test_storage();
    let timestamp = now();

    // Contact has 3 devices
    let devices = vec!["device-1", "device-2", "device-3"];

    for device_id in &devices {
        let record = DeviceDeliveryRecord {
            message_id: "msg-001".to_string(),
            recipient_id: "contact-abc".to_string(),
            device_id: device_id.to_string(),
            status: DeviceDeliveryStatus::Pending,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    // Get all device records for message
    let records = storage.get_device_deliveries_for_message("msg-001").unwrap();
    assert_eq!(records.len(), 3);
}

#[test]
fn test_update_device_delivery_status() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeviceDeliveryRecord {
        message_id: "msg-002".to_string(),
        recipient_id: "contact-xyz".to_string(),
        device_id: "device-1".to_string(),
        status: DeviceDeliveryStatus::Pending,
        updated_at: timestamp,
    };
    storage.create_device_delivery(&record).unwrap();

    // Update to Delivered
    storage
        .update_device_delivery_status(
            "msg-002",
            "device-1",
            DeviceDeliveryStatus::Delivered,
            timestamp + 1,
        )
        .unwrap();

    let retrieved = storage
        .get_device_delivery("msg-002", "device-1")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.status, DeviceDeliveryStatus::Delivered);
    assert_eq!(retrieved.updated_at, timestamp + 1);
}

#[test]
fn test_get_delivery_summary() {
    let storage = test_storage();
    let timestamp = now();

    // Message to contact with 3 devices
    let devices = vec![
        ("device-1", DeviceDeliveryStatus::Delivered),
        ("device-2", DeviceDeliveryStatus::Delivered),
        ("device-3", DeviceDeliveryStatus::Pending),
    ];

    for (device_id, status) in devices {
        let record = DeviceDeliveryRecord {
            message_id: "msg-003".to_string(),
            recipient_id: "contact-abc".to_string(),
            device_id: device_id.to_string(),
            status,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    // Get summary: "Delivered to X of Y devices"
    let summary = storage.get_delivery_summary("msg-003").unwrap();
    assert_eq!(summary.total_devices, 3);
    assert_eq!(summary.delivered_devices, 2);
    assert!(!summary.is_fully_delivered());
}

#[test]
fn test_is_fully_delivered() {
    let storage = test_storage();
    let timestamp = now();

    // All devices delivered
    for i in 0..2 {
        let record = DeviceDeliveryRecord {
            message_id: "msg-full".to_string(),
            recipient_id: "contact".to_string(),
            device_id: format!("device-{}", i),
            status: DeviceDeliveryStatus::Delivered,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    let summary = storage.get_delivery_summary("msg-full").unwrap();
    assert!(summary.is_fully_delivered());
}

#[test]
fn test_delete_device_deliveries_for_message() {
    let storage = test_storage();
    let timestamp = now();

    // Create records for a message
    for i in 0..3 {
        let record = DeviceDeliveryRecord {
            message_id: "msg-delete".to_string(),
            recipient_id: "contact".to_string(),
            device_id: format!("device-{}", i),
            status: DeviceDeliveryStatus::Pending,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    assert_eq!(
        storage
            .get_device_deliveries_for_message("msg-delete")
            .unwrap()
            .len(),
        3
    );

    // Delete all for message
    let deleted = storage
        .delete_device_deliveries_for_message("msg-delete")
        .unwrap();
    assert_eq!(deleted, 3);

    assert_eq!(
        storage
            .get_device_deliveries_for_message("msg-delete")
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_get_pending_device_deliveries() {
    let storage = test_storage();
    let timestamp = now();

    // Mix of pending and delivered
    let records = vec![
        ("msg-1", "device-a", DeviceDeliveryStatus::Pending),
        ("msg-1", "device-b", DeviceDeliveryStatus::Delivered),
        ("msg-2", "device-a", DeviceDeliveryStatus::Pending),
        ("msg-2", "device-b", DeviceDeliveryStatus::Pending),
    ];

    for (msg_id, device_id, status) in records {
        let record = DeviceDeliveryRecord {
            message_id: msg_id.to_string(),
            recipient_id: "contact".to_string(),
            device_id: device_id.to_string(),
            status,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    // Get all pending
    let pending = storage.get_pending_device_deliveries().unwrap();
    assert_eq!(pending.len(), 3); // 3 pending records
}

#[test]
fn test_device_delivery_status_transitions() {
    let storage = test_storage();
    let timestamp = now();

    let record = DeviceDeliveryRecord {
        message_id: "msg-trans".to_string(),
        recipient_id: "contact".to_string(),
        device_id: "device-1".to_string(),
        status: DeviceDeliveryStatus::Pending,
        updated_at: timestamp,
    };
    storage.create_device_delivery(&record).unwrap();

    // Pending -> Stored
    storage
        .update_device_delivery_status(
            "msg-trans",
            "device-1",
            DeviceDeliveryStatus::Stored,
            timestamp + 1,
        )
        .unwrap();

    let r = storage
        .get_device_delivery("msg-trans", "device-1")
        .unwrap()
        .unwrap();
    assert_eq!(r.status, DeviceDeliveryStatus::Stored);

    // Stored -> Delivered
    storage
        .update_device_delivery_status(
            "msg-trans",
            "device-1",
            DeviceDeliveryStatus::Delivered,
            timestamp + 2,
        )
        .unwrap();

    let r = storage
        .get_device_delivery("msg-trans", "device-1")
        .unwrap()
        .unwrap();
    assert_eq!(r.status, DeviceDeliveryStatus::Delivered);
}

#[test]
fn test_count_device_deliveries_by_status() {
    let storage = test_storage();
    let timestamp = now();

    let records = vec![
        ("msg-1", "dev-1", DeviceDeliveryStatus::Pending),
        ("msg-1", "dev-2", DeviceDeliveryStatus::Pending),
        ("msg-2", "dev-1", DeviceDeliveryStatus::Delivered),
        ("msg-2", "dev-2", DeviceDeliveryStatus::Failed),
    ];

    for (msg_id, device_id, status) in records {
        let record = DeviceDeliveryRecord {
            message_id: msg_id.to_string(),
            recipient_id: "contact".to_string(),
            device_id: device_id.to_string(),
            status,
            updated_at: timestamp,
        };
        storage.create_device_delivery(&record).unwrap();
    }

    assert_eq!(
        storage
            .count_device_deliveries_by_status(DeviceDeliveryStatus::Pending)
            .unwrap(),
        2
    );
    assert_eq!(
        storage
            .count_device_deliveries_by_status(DeviceDeliveryStatus::Delivered)
            .unwrap(),
        1
    );
    assert_eq!(
        storage
            .count_device_deliveries_by_status(DeviceDeliveryStatus::Failed)
            .unwrap(),
        1
    );
}

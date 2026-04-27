// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the delivery-status FFI surface — specifically the failed-record
//! partition method that frontends consume in place of `.filter(\.isFailed)`.
//!
//! Closes the symmetric Humble-UI violation tracked in
//! `_private/docs/problems/2026-04-27-screenmodel-api-gaps-symmetric-frontend-violations`
//! (G3). The substantive partition logic is covered at the storage layer in
//! `core/vauchi-core/tests/it/delivery_storage_tests.rs`; these tests verify
//! the platform delegation, type mapping, and FFI surface stability.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_platform::{MobileDeliveryStatus, VauchiPlatform};

fn setup() -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().unwrap();
    let wb = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .unwrap();
    wb.create_identity("Alice".to_string()).unwrap();
    (wb, dir)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn make_record(message_id: &str, status: DeliveryStatus) -> DeliveryRecord {
    let ts = now();
    DeliveryRecord {
        message_id: message_id.to_string(),
        recipient_id: "contact-1".to_string(),
        status,
        // Use a recent timestamp so the 30-day startup-maintenance sweep
        // (Storage::open → run_startup_maintenance) doesn't delete terminal
        // records before the query runs.
        created_at: ts,
        updated_at: ts,
        expires_at: None,
    }
}

// @internal
#[test]
fn get_failed_delivery_records_returns_empty_when_no_records() {
    let (wb, _dir) = setup();
    let records = wb
        .get_failed_delivery_records()
        .expect("call must succeed on empty store");
    assert!(
        records.is_empty(),
        "empty store must return zero failed records, got {}",
        records.len()
    );
}

// @internal
#[test]
fn get_failed_delivery_records_returns_only_failed_records() {
    let (wb, _dir) = setup();

    wb.save_test_delivery_record(&make_record(
        "msg-failed",
        DeliveryStatus::Failed {
            reason: "network".to_string(),
        },
    ))
    .unwrap();
    wb.save_test_delivery_record(&make_record("msg-delivered", DeliveryStatus::Delivered))
        .unwrap();
    wb.save_test_delivery_record(&make_record("msg-queued", DeliveryStatus::Queued))
        .unwrap();

    let records = wb.get_failed_delivery_records().unwrap();
    assert_eq!(
        records.len(),
        1,
        "must return exactly one failed record (got {})",
        records.len()
    );
    assert_eq!(
        records[0].message_id, "msg-failed",
        "must be the failed-status record"
    );
    assert_eq!(
        records[0].status,
        MobileDeliveryStatus::Failed,
        "MobileDeliveryStatus must reflect the underlying DeliveryStatus::Failed variant"
    );
}

// @internal
#[test]
fn get_failed_delivery_records_excludes_other_terminal_statuses() {
    let (wb, _dir) = setup();

    wb.save_test_delivery_record(&make_record("msg-expired", DeliveryStatus::Expired))
        .unwrap();
    wb.save_test_delivery_record(&make_record("msg-delivered", DeliveryStatus::Delivered))
        .unwrap();

    let records = wb.get_failed_delivery_records().unwrap();
    assert!(
        records.is_empty(),
        "Expired and Delivered are terminal but not Failed; expected zero, got {}",
        records.len()
    );
}

// @internal
#[test]
fn get_failed_delivery_records_returns_multiple_when_present() {
    let (wb, _dir) = setup();

    for i in 0..3 {
        wb.save_test_delivery_record(&make_record(
            &format!("msg-{i}"),
            DeliveryStatus::Failed {
                reason: "timeout".to_string(),
            },
        ))
        .unwrap();
    }

    let records = wb.get_failed_delivery_records().unwrap();
    assert_eq!(
        records.len(),
        3,
        "all three Failed records must be returned"
    );
    for record in &records {
        assert_eq!(
            record.status,
            MobileDeliveryStatus::Failed,
            "every returned record must have Failed status"
        );
    }
}

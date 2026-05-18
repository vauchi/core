// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the delivery-status FFI surface — specifically the failed-record
//! partition that frontends consume via `DomainCommand::GetFailedDeliveryRecords`
//! in place of `.filter(\.isFailed)`.
//!
//! Closes the symmetric Humble-UI violation tracked in
//! `_private/docs/problems/2026-04-27-screenmodel-api-gaps-symmetric-frontend-violations`
//! (G3). The substantive partition logic is covered at the storage layer in
//! `core/vauchi-core/tests/it/delivery_storage_tests.rs`; these tests verify
//! the platform delegation, type mapping, and FFI surface stability through
//! the `PlatformAppEngine` dispatch path.
//!
//! Migrated 2026-05-18 from `VauchiPlatform.get_failed_delivery_records()`
//! to `PlatformAppEngine.dispatch_domain_command(DomainCommand::GetFailedDeliveryRecords)`
//! during Phase 2a slice 32h.Ph2 retirement.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileDeliveryRecord, MobileDeliveryStatus,
    PlatformAppEngine, PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    drive_onboarding(&engine);
    (engine, dir)
}

/// Drive the onboarding flow to create the identity. Mirrors the pattern in
/// `contact_lifecycle_tests.rs` and `platform_app_engine_domain_command_tests.rs`.
fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
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

fn get_failed_records(engine: &PlatformAppEngine) -> Vec<MobileDeliveryRecord> {
    match engine
        .dispatch_domain_command(DomainCommand::GetFailedDeliveryRecords)
        .expect("GetFailedDeliveryRecords dispatch")
    {
        DomainCommandResult::DeliveryRecords { records } => records,
        other => panic!("expected DeliveryRecords, got {other:?}"),
    }
}

// @internal
#[test]
fn get_failed_delivery_records_returns_empty_when_no_records() {
    let (engine, _dir) = setup();
    let records = get_failed_records(&engine);
    assert!(
        records.is_empty(),
        "empty store must return zero failed records, got {}",
        records.len()
    );
}

// @internal
#[test]
fn get_failed_delivery_records_returns_only_failed_records() {
    let (engine, _dir) = setup();

    engine
        .save_test_delivery_record(&make_record(
            "msg-failed",
            DeliveryStatus::Failed {
                reason: "network".to_string(),
            },
        ))
        .unwrap();
    engine
        .save_test_delivery_record(&make_record("msg-delivered", DeliveryStatus::Delivered))
        .unwrap();
    engine
        .save_test_delivery_record(&make_record("msg-queued", DeliveryStatus::Queued))
        .unwrap();

    let records = get_failed_records(&engine);
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
    let (engine, _dir) = setup();

    engine
        .save_test_delivery_record(&make_record("msg-expired", DeliveryStatus::Expired))
        .unwrap();
    engine
        .save_test_delivery_record(&make_record("msg-delivered", DeliveryStatus::Delivered))
        .unwrap();

    let records = get_failed_records(&engine);
    assert!(
        records.is_empty(),
        "Expired and Delivered are terminal but not Failed; expected zero, got {}",
        records.len()
    );
}

// @internal
#[test]
fn get_failed_delivery_records_returns_multiple_when_present() {
    let (engine, _dir) = setup();

    for i in 0..3 {
        engine
            .save_test_delivery_record(&make_record(
                &format!("msg-{i}"),
                DeliveryStatus::Failed {
                    reason: "timeout".to_string(),
                },
            ))
            .unwrap();
    }

    let records = get_failed_records(&engine);
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

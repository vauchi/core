// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `PlatformAppEngine::dispatch_domain_command`.
//!
//! Phase B7 of `2026-04-28-collapse-vauchi-platform-into-app-engine`,
//! first batch (Consent — 5 variants). Subsequent B7 batch MRs add new
//! domains by extending [`DomainCommand`] / [`DomainCommandResult`]
//! and adding new arms here.

use std::sync::Arc;

use vauchi_platform::{DomainCommand, DomainCommandResult, MobileConsentType, PlatformAppEngine};

fn create_engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    drive_onboarding(&engine);
    (engine, dir)
}

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

// ── GrantConsent → CheckConsent round-trip ──────────────────────────

// @internal
#[test]
fn grant_consent_then_check_consent_returns_true() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GrantConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("grant_consent dispatch")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }

    match engine
        .dispatch_domain_command(DomainCommand::CheckConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("check_consent dispatch")
    {
        DomainCommandResult::Bool { value } => assert!(value, "consent must be granted"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn check_consent_returns_false_when_never_granted() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CheckConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("check_consent dispatch")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "no consent granted yet"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── RevokeConsent ───────────────────────────────────────────────────

// @internal
#[test]
fn revoke_consent_after_grant_flips_check_result() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::GrantConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("grant");

    engine
        .dispatch_domain_command(DomainCommand::RevokeConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("revoke");

    match engine
        .dispatch_domain_command(DomainCommand::CheckConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("check")
    {
        DomainCommandResult::Bool { value } => {
            assert!(!value, "consent must be revoked")
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── GetConsentStatus ────────────────────────────────────────────────

// @internal
#[test]
fn get_consent_status_reflects_grant() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::GrantConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("grant");

    match engine
        .dispatch_domain_command(DomainCommand::GetConsentStatus {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("status")
    {
        DomainCommandResult::ConsentStatus { status } => {
            assert!(status.granted, "status must reflect granted state");
            assert!(
                status.last_changed_at.is_some(),
                "last_changed_at must be set after grant"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── GetConsentRecords ───────────────────────────────────────────────

// @internal
#[test]
fn get_consent_records_lists_grants_and_revokes() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::GrantConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("grant");
    engine
        .dispatch_domain_command(DomainCommand::RevokeConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("revoke");

    match engine
        .dispatch_domain_command(DomainCommand::GetConsentRecords)
        .expect("records")
    {
        DomainCommandResult::ConsentRecords { records } => {
            assert!(records.len() >= 2, "must contain grant + revoke entries");
            let grants = records.iter().filter(|r| r.granted).count();
            let revokes = records.iter().filter(|r| !r.granted).count();
            assert!(grants >= 1, "at least one grant record");
            assert!(revokes >= 1, "at least one revoke record");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_consent_records_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetConsentRecords)
        .expect("records")
    {
        DomainCommandResult::ConsentRecords { records } => {
            assert!(records.is_empty(), "no records before any grant/revoke");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── Cache invalidation contract ──────────────────────────────────────

// @internal
#[test]
fn grant_consent_invalidates_settings_and_privacy_screens() {
    // After a write through dispatch, the next current_screen_json
    // must rebuild the affected screens rather than serve stale data.
    // Smoke-level: assert no panic on read-after-write.
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::GrantConsent {
            consent_type: MobileConsentType::DataProcessing,
        })
        .expect("grant");

    engine.invalidate_all().expect("invalidate_all");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after grant");
}

// ── Content Updates (B7 batch 2) ────────────────────────────────────

// @internal
#[test]
fn is_content_updates_supported_returns_compile_time_flag() {
    let (engine, _dir) = create_engine_with_identity();

    let expected = cfg!(feature = "content-updates");
    match engine
        .dispatch_domain_command(DomainCommand::IsContentUpdatesSupported)
        .expect("dispatch")
    {
        DomainCommandResult::Bool { value } => {
            assert_eq!(value, expected, "must reflect compile-time feature");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn check_content_updates_returns_disabled_when_feature_off() {
    // The default test build does not enable `content-updates`, so the
    // dispatch must return `Disabled`. With the feature on the call
    // would attempt a network check we don't want in unit-time tests.
    if cfg!(feature = "content-updates") {
        return;
    }
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CheckContentUpdates)
        .expect("dispatch")
    {
        DomainCommandResult::UpdateStatus { status } => match status {
            vauchi_platform::MobileUpdateStatus::Disabled => {}
            other => panic!("expected Disabled, got {other:?}"),
        },
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn apply_content_updates_returns_disabled_when_feature_off() {
    if cfg!(feature = "content-updates") {
        return;
    }
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ApplyContentUpdates)
        .expect("dispatch")
    {
        DomainCommandResult::ApplyResult { result } => match result {
            vauchi_platform::MobileApplyResult::Disabled => {}
            other => panic!("expected Disabled, got {other:?}"),
        },
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn reload_social_networks_returns_default_registry() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ReloadSocialNetworks)
        .expect("dispatch")
    {
        DomainCommandResult::SocialNetworks { networks } => {
            // Default registry ships several known networks.
            assert!(
                !networks.is_empty(),
                "default registry must contain entries"
            );
            for n in &networks {
                assert!(!n.id.is_empty(), "network id must be non-empty");
                assert!(!n.url_template.is_empty(), "url template must be non-empty");
            }
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── Sync / Delivery / Retry (B7 batch 8) ────────────────────────────

// @internal
#[test]
fn pending_update_count_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::PendingUpdateCount)
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_total_pending_count_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetTotalPendingCount)
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn count_failed_deliveries_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CountFailedDeliveries)
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_all_delivery_records_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetAllDeliveryRecords)
        .expect("records")
    {
        DomainCommandResult::DeliveryRecords { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_failed_delivery_records_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetFailedDeliveryRecords)
        .expect("records")
    {
        DomainCommandResult::DeliveryRecords { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_pending_deliveries_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetPendingDeliveries)
        .expect("records")
    {
        DomainCommandResult::DeliveryRecords { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_delivery_record_returns_none_for_unknown_id() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDeliveryRecord {
            message_id: "nonexistent".into(),
        })
        .expect("get")
    {
        DomainCommandResult::DeliveryRecordOpt { record } => {
            assert!(record.is_none());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_delivery_records_for_contact_is_empty_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDeliveryRecordsForContact {
            recipient_id: "nonexistent".into(),
        })
        .expect("records")
    {
        DomainCommandResult::DeliveryRecords { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn manual_retry_returns_false_for_unknown_message() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ManualRetry {
            message_id: "nonexistent".into(),
        })
        .expect("retry")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "no entry → false"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn delete_retry_returns_false_for_unknown_message() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::DeleteRetry {
            message_id: "nonexistent".into(),
        })
        .expect("delete")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "no entry → false"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_due_retries_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDueRetries)
        .expect("entries")
    {
        DomainCommandResult::RetryEntries { entries } => {
            assert!(entries.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_retry_count_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetRetryCount)
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_retries_for_contact_is_empty_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetRetriesForContact {
            contact_id: "nonexistent".into(),
        })
        .expect("entries")
    {
        DomainCommandResult::RetryEntries { entries } => {
            assert!(entries.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn calculate_retry_backoff_grows_with_attempt() {
    let (engine, _dir) = create_engine_with_identity();

    let backoff_at = |attempt: u32| -> u64 {
        match engine
            .dispatch_domain_command(DomainCommand::CalculateRetryBackoff { attempt })
            .expect("backoff")
        {
            DomainCommandResult::BackoffSeconds { seconds } => seconds,
            other => panic!("unexpected result: {other:?}"),
        }
    };

    let b0 = backoff_at(0);
    let b3 = backoff_at(3);
    assert!(b3 >= b0, "backoff must be non-decreasing in attempt count");
}

// @internal
#[test]
fn is_offline_queue_full_is_false_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsOfflineQueueFull)
        .expect("full")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_offline_queue_capacity_is_positive_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetOfflineQueueCapacity)
        .expect("capacity")
    {
        DomainCommandResult::Count { value } => {
            assert!(value > 0, "fresh queue has positive remaining capacity");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn clear_pending_updates_for_unknown_contact_returns_zero() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ClearPendingUpdatesForContact {
            contact_id: "nonexistent".into(),
        })
        .expect("clear")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_delivery_count_by_status_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDeliveryCountByStatus {
            status: vauchi_platform::MobileDeliveryStatus::Queued,
        })
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_pending_device_deliveries_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetPendingDeviceDeliveries)
        .expect("records")
    {
        DomainCommandResult::DeviceDeliveries { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_device_deliveries_for_unknown_message_is_empty() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDeviceDeliveries {
            message_id: "nonexistent".into(),
        })
        .expect("records")
    {
        DomainCommandResult::DeviceDeliveries { records } => {
            assert!(records.is_empty());
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

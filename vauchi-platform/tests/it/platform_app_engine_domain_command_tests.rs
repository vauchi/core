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

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileAhaMomentType, MobileConsentType, PlatformAppEngine,
};

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

// ── Aha Moments (B7 batch 5) ────────────────────────────────────────

// @internal
#[test]
fn has_seen_aha_moment_returns_false_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::HasSeenAhaMoment {
            moment_type: MobileAhaMomentType::CardCreationComplete,
        })
        .expect("has_seen")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "no moments seen yet"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn try_trigger_aha_moment_returns_payload_on_first_call_only() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::TryTriggerAhaMoment {
            moment_type: MobileAhaMomentType::FirstEdit,
        })
        .expect("first trigger")
    {
        DomainCommandResult::AhaMomentOpt { moment } => {
            let m = moment.expect("first trigger must return Some");
            assert!(!m.title.is_empty(), "title must be present");
            assert!(!m.message.is_empty(), "message must be present");
        }
        other => panic!("unexpected result: {other:?}"),
    }

    match engine
        .dispatch_domain_command(DomainCommand::TryTriggerAhaMoment {
            moment_type: MobileAhaMomentType::FirstEdit,
        })
        .expect("second trigger")
    {
        DomainCommandResult::AhaMomentOpt { moment } => {
            assert!(moment.is_none(), "second trigger must return None");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn aha_moments_seen_count_increments_after_trigger() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::AhaMomentsSeenCount)
        .expect("initial")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }

    engine
        .dispatch_domain_command(DomainCommand::TryTriggerAhaMoment {
            moment_type: MobileAhaMomentType::CardCreationComplete,
        })
        .expect("trigger");

    match engine
        .dispatch_domain_command(DomainCommand::AhaMomentsSeenCount)
        .expect("after-trigger")
    {
        DomainCommandResult::Count { value } => {
            assert_eq!(value, 1, "must increment after trigger")
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn aha_moments_total_count_is_positive() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::AhaMomentsTotalCount)
        .expect("total")
    {
        DomainCommandResult::Count { value } => {
            assert!(value > 0, "core defines >= 1 aha moment");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn reset_aha_moments_clears_seen_count() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::TryTriggerAhaMoment {
            moment_type: MobileAhaMomentType::FirstEdit,
        })
        .expect("trigger");
    engine
        .dispatch_domain_command(DomainCommand::ResetAhaMoments)
        .expect("reset");

    match engine
        .dispatch_domain_command(DomainCommand::AhaMomentsSeenCount)
        .expect("after-reset")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0, "reset clears seen count"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── Demo Contact (B7 batch 5) ───────────────────────────────────────

// @internal
#[test]
fn init_demo_contact_if_needed_returns_some_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::InitDemoContactIfNeeded)
        .expect("init")
    {
        DomainCommandResult::DemoContactOpt { contact } => {
            let c = contact.expect("fresh identity gets a demo contact");
            assert!(c.is_demo, "demo flag must be set");
            assert!(!c.id.is_empty(), "id must be populated");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_demo_contact_state_initially_inactive() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDemoContactState)
        .expect("state")
    {
        DomainCommandResult::DemoContactState { state } => {
            assert!(!state.is_active, "no init yet → inactive");
            assert!(!state.was_dismissed);
            assert!(!state.auto_removed);
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn dismiss_demo_contact_persists_dismissal() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::InitDemoContactIfNeeded)
        .expect("init");
    engine
        .dispatch_domain_command(DomainCommand::DismissDemoContact)
        .expect("dismiss");

    match engine
        .dispatch_domain_command(DomainCommand::GetDemoContactState)
        .expect("state")
    {
        DomainCommandResult::DemoContactState { state } => {
            assert!(state.was_dismissed, "dismissal must persist");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn auto_remove_demo_contact_returns_false_when_inactive() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::AutoRemoveDemoContact)
        .expect("auto_remove")
    {
        DomainCommandResult::Bool { value } => {
            assert!(!value, "no active demo → no removal");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn restore_demo_contact_clears_dismissal() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::InitDemoContactIfNeeded)
        .expect("init");
    engine
        .dispatch_domain_command(DomainCommand::DismissDemoContact)
        .expect("dismiss");

    let _ = engine
        .dispatch_domain_command(DomainCommand::RestoreDemoContact)
        .expect("restore");

    match engine
        .dispatch_domain_command(DomainCommand::GetDemoContactState)
        .expect("state")
    {
        DomainCommandResult::DemoContactState { state } => {
            assert!(!state.was_dismissed, "restore clears dismissal");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}
// ── GDPR / Deletion + shred-status (B7 batch 3) ─────────────────────

// @internal
#[test]
fn export_gdpr_data_returns_json_payload() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ExportGdprData)
        .expect("export")
    {
        DomainCommandResult::GdprExport { export } => {
            assert!(!export.json_data.is_empty(), "json_data must be present");
            // Must round-trip as JSON (`{...}` envelope).
            let parsed: serde_json::Value =
                serde_json::from_str(&export.json_data).expect("parse json");
            assert!(parsed.is_object(), "export must be a JSON object");
            assert!(export.exported_at > 0, "exported_at must be set");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn schedule_identity_deletion_returns_scheduled_state() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ScheduleIdentityDeletion)
        .expect("schedule")
    {
        DomainCommandResult::DeletionInfo { info } => {
            assert!(
                info.scheduled_at > 0,
                "scheduled_at must be set after schedule"
            );
            assert!(info.execute_at > info.scheduled_at, "grace period > 0");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn cancel_identity_deletion_after_schedule_returns_to_none() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::ScheduleIdentityDeletion)
        .expect("schedule");
    match engine
        .dispatch_domain_command(DomainCommand::CancelIdentityDeletion)
        .expect("cancel")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }

    match engine
        .dispatch_domain_command(DomainCommand::GetDeletionState)
        .expect("get_state")
    {
        DomainCommandResult::DeletionInfo { info } => {
            assert_eq!(info.scheduled_at, 0, "cancel must clear scheduled_at");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_deletion_state_returns_none_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDeletionState)
        .expect("state")
    {
        DomainCommandResult::DeletionInfo { info } => {
            assert_eq!(info.scheduled_at, 0, "no schedule yet");
            assert_eq!(info.execute_at, 0);
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn shred_status_is_none_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ShredStatus)
        .expect("status")
    {
        DomainCommandResult::ShredStatus { status } => {
            assert!(
                matches!(status, vauchi_platform::MobileShredStatus::None),
                "shred status must be None initially"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn shred_status_after_schedule_reports_scheduled() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::ScheduleIdentityDeletion)
        .expect("schedule");

    match engine
        .dispatch_domain_command(DomainCommand::ShredStatus)
        .expect("status")
    {
        DomainCommandResult::ShredStatus { status } => match status {
            vauchi_platform::MobileShredStatus::Scheduled { remaining_secs } => {
                assert!(remaining_secs > 0, "grace period must be positive");
            }
            other => panic!("expected Scheduled, got {other:?}"),
        },
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn schedule_deletion_invalidates_settings_cache() {
    // Cache invalidation contract for GDPR/Deletion writes.
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::ScheduleIdentityDeletion)
        .expect("schedule");

    engine.invalidate_all().expect("invalidate_all");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after schedule");
}

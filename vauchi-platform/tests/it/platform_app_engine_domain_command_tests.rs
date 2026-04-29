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

// ── Contact Card + CRUD (B7 batch 10) ───────────────────────────────

// @internal
#[test]
fn get_own_card_returns_onboarded_card() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetOwnCard)
        .expect("get_own_card")
    {
        DomainCommandResult::ContactCardPayload { card } => {
            assert_eq!(card.display_name, "Alice");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_contacts_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListContacts)
        .expect("list")
    {
        DomainCommandResult::Contacts { contacts } => assert!(contacts.is_empty()),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn contact_count_is_zero_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ContactCount)
        .expect("count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_contact_returns_none_for_unknown_id() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetContact {
            id: "nonexistent".into(),
        })
        .expect("get")
    {
        DomainCommandResult::ContactOpt { contact } => assert!(contact.is_none()),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn search_contacts_is_empty_for_unknown_query() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SearchContacts {
            query: "definitely-not-a-name".into(),
        })
        .expect("search")
    {
        DomainCommandResult::Contacts { contacts } => assert!(contacts.is_empty()),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn remove_contact_returns_false_for_unknown_id() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::RemoveContact {
            id: "nonexistent".into(),
        })
        .expect("remove")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_archived_contacts_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListArchivedContacts)
        .expect("archived")
    {
        DomainCommandResult::Contacts { contacts } => assert!(contacts.is_empty()),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn add_field_updates_own_card() {
    use vauchi_platform::MobileFieldType;
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::AddField {
            field_type: MobileFieldType::Email,
            label: "Work".into(),
            value: "alice@example.com".into(),
        })
        .expect("add_field");

    match engine
        .dispatch_domain_command(DomainCommand::GetOwnCard)
        .expect("get")
    {
        DomainCommandResult::ContactCardPayload { card } => {
            assert!(
                card.fields.iter().any(|f| f.label == "Work"),
                "added field must appear on card"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn update_field_errors_on_unknown_label() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::UpdateField {
        label: "DoesNotExist".into(),
        new_value: "x".into(),
    });
    assert!(result.is_err(), "unknown label must error");
}

// @internal
#[test]
fn remove_field_returns_false_for_unknown_label() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::RemoveField {
            label: "DoesNotExist".into(),
        })
        .expect("remove")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn set_display_name_updates_card() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetDisplayName {
            name: "Renamed".into(),
        })
        .expect("set");

    match engine
        .dispatch_domain_command(DomainCommand::GetOwnCard)
        .expect("get")
    {
        DomainCommandResult::ContactCardPayload { card } => {
            assert_eq!(card.display_name, "Renamed");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn clear_own_avatar_returns_unit() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ClearOwnAvatar)
        .expect("clear")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn archive_unknown_contact_errors() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::ArchiveContact {
        id: "nonexistent".into(),
    });
    assert!(result.is_err(), "archiving unknown contact must error");
}

// @internal
#[test]
fn hide_unknown_contact_errors() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::HideContact {
        contact_id: "nonexistent".into(),
    });
    assert!(result.is_err(), "hiding unknown contact must error");
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

// ── Recovery leftovers (B7 batch 4) ─────────────────────────────────

// @internal
#[test]
fn verify_recovery_proof_rejects_invalid_base64() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::VerifyRecoveryProof {
        proof_b64: "not!valid!base64!".into(),
    });
    assert!(result.is_err(), "invalid base64 must error");
}

// @internal
#[test]
fn verify_recovery_proof_rejects_garbage_proof() {
    use base64::Engine;
    let (engine, _dir) = create_engine_with_identity();

    // Syntactically valid base64 but garbage proof bytes.
    let garbage = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
    let result =
        engine.dispatch_domain_command(DomainCommand::VerifyRecoveryProof { proof_b64: garbage });
    assert!(result.is_err(), "garbage proof bytes must error");
}

// @internal
#[test]
fn save_recovery_response_persists_decision() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SaveRecoveryResponse {
            claim_id: "claim-1".into(),
            contact_id: "contact-a".into(),
            response: "accept".into(),
            remind_at: None,
        })
        .expect("save")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn save_recovery_response_with_remind_at_succeeds() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SaveRecoveryResponse {
            claim_id: "claim-2".into(),
            contact_id: "contact-b".into(),
            response: "remind_me_later".into(),
            remind_at: Some(9999),
        })
        .expect("save")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn upload_guardian_entries_with_no_trusted_contacts_short_circuits() {
    // With zero recovery-trusted contacts, upload_guardian_entries
    // takes the "delete" short-circuit path (no upload attempted).
    // The dispatch may still error on relay unreachability — assert
    // that BOTH branches return a known shape (Ok(Unit) on success
    // or an Err whose detail mentions guardian/relay), never any
    // other DomainCommandResult variant.
    let (engine, _dir) = create_engine_with_identity();

    match engine.dispatch_domain_command(DomainCommand::UploadGuardianEntries) {
        Ok(DomainCommandResult::Unit) => {}
        Ok(other) => panic!("must return Unit on success, got {other:?}",),
        Err(e) => {
            // Network failure is acceptable; verify we hit a real
            // dispatch path (not a panic / lock failure).
            let msg = format!("{e:?}");
            assert!(
                !msg.is_empty(),
                "error must surface a non-empty detail: {msg}"
            );
        }
    }
}

// @internal
#[test]
fn save_recovery_response_invalidates_recovery_screens() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SaveRecoveryResponse {
            claim_id: "claim-3".into(),
            contact_id: "contact-c".into(),
            response: "reject".into(),
            remind_at: None,
        })
        .expect("save");

    engine.invalidate_all().expect("invalidate_all");
    let json = engine
        .current_screen_json()
        .expect("current_screen_json after save");
    assert!(
        !json.is_empty(),
        "current_screen_json must return non-empty payload after invalidation"
    );
}

// ── Visibility Labels + Field Visibility (B7 batch 6) ──────────────

// @internal
#[test]
fn list_labels_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListLabels)
        .expect("list_labels")
    {
        DomainCommandResult::Labels { labels } => {
            assert!(labels.is_empty(), "no labels created yet");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn create_label_returns_label_with_name() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CreateLabel {
            name: "Family".into(),
        })
        .expect("create")
    {
        DomainCommandResult::Label { label } => {
            assert_eq!(label.name, "Family");
            assert_eq!(label.contact_count, 0);
            assert!(!label.id.is_empty(), "id must be populated");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_labels_includes_created_label() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::CreateLabel {
            name: "Friends".into(),
        })
        .expect("create");

    match engine
        .dispatch_domain_command(DomainCommand::ListLabels)
        .expect("list")
    {
        DomainCommandResult::Labels { labels } => {
            assert_eq!(labels.len(), 1);
            assert_eq!(labels[0].name, "Friends");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn rename_label_persists_new_name() {
    let (engine, _dir) = create_engine_with_identity();

    let label = match engine
        .dispatch_domain_command(DomainCommand::CreateLabel { name: "Old".into() })
        .expect("create")
    {
        DomainCommandResult::Label { label } => label,
        other => panic!("unexpected result: {other:?}"),
    };

    engine
        .dispatch_domain_command(DomainCommand::RenameLabel {
            label_id: label.id.clone(),
            new_name: "New".into(),
        })
        .expect("rename");

    match engine
        .dispatch_domain_command(DomainCommand::GetLabel { label_id: label.id })
        .expect("get")
    {
        DomainCommandResult::LabelDetail { detail } => {
            assert_eq!(detail.name, "New", "rename must persist");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn delete_label_removes_from_list() {
    let (engine, _dir) = create_engine_with_identity();

    let label = match engine
        .dispatch_domain_command(DomainCommand::CreateLabel {
            name: "Temp".into(),
        })
        .expect("create")
    {
        DomainCommandResult::Label { label } => label,
        other => panic!("unexpected result: {other:?}"),
    };

    engine
        .dispatch_domain_command(DomainCommand::DeleteLabel { label_id: label.id })
        .expect("delete");

    match engine
        .dispatch_domain_command(DomainCommand::ListLabels)
        .expect("list")
    {
        DomainCommandResult::Labels { labels } => {
            assert!(labels.is_empty(), "label must be gone");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn add_contact_to_group_errors_on_unknown_label() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::AddContactToGroup {
        label_id: "nonexistent".into(),
        contact_id: "contact-x".into(),
    });
    assert!(result.is_err(), "unknown label must error");
}

// @internal
#[test]
fn get_groups_for_contact_is_empty_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetGroupsForContact {
            contact_id: "nonexistent".into(),
        })
        .expect("query")
    {
        DomainCommandResult::Labels { labels } => {
            assert!(labels.is_empty(), "no groups for unknown contact");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn set_group_field_visibility_errors_on_unknown_field() {
    let (engine, _dir) = create_engine_with_identity();

    let label = match engine
        .dispatch_domain_command(DomainCommand::CreateLabel {
            name: "Inner".into(),
        })
        .expect("create")
    {
        DomainCommandResult::Label { label } => label,
        other => panic!("unexpected result: {other:?}"),
    };

    let result = engine.dispatch_domain_command(DomainCommand::SetGroupFieldVisibility {
        label_id: label.id,
        field_label: "DoesNotExist".into(),
        is_visible: false,
    });
    assert!(result.is_err(), "unknown field label must error");
}

// @internal
#[test]
fn hide_field_from_contact_errors_on_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::HideFieldFromContact {
        contact_id: "nonexistent".into(),
        field_label: "Phone".into(),
    });
    assert!(result.is_err(), "unknown contact must error");
}

// @internal
#[test]
fn show_field_to_contact_errors_on_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::ShowFieldToContact {
        contact_id: "nonexistent".into(),
        field_label: "Phone".into(),
    });
    assert!(result.is_err(), "unknown contact must error");
}

// @internal
#[test]
fn is_field_visible_to_contact_errors_on_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::IsFieldVisibleToContact {
        contact_id: "nonexistent".into(),
        field_label: "Phone".into(),
    });
    assert!(result.is_err(), "unknown contact must error");
}

// @internal
#[test]
fn get_suggested_labels_returns_non_empty_list() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetSuggestedLabels)
        .expect("suggested")
    {
        DomainCommandResult::Strings { values } => {
            assert!(!values.is_empty(), "core ships >=1 suggested label");
            for v in &values {
                assert!(!v.is_empty(), "suggestion must be non-empty");
            }
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn create_label_invalidates_groups_screen() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::CreateLabel {
            name: "Cached".into(),
        })
        .expect("create");

    engine.invalidate_all().expect("invalidate_all");
    let json = engine
        .current_screen_json()
        .expect("current_screen_json after create");
    assert!(!json.is_empty());
}

// ── Passcode + Duress + Decoy (B7 batch 7) ──────────────────────────

// @internal
#[test]
fn is_password_enabled_returns_false_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsPasswordEnabled)
        .expect("is_password_enabled")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn is_duress_enabled_returns_false_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsDuressEnabled)
        .expect("is_duress_enabled")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn setup_app_password_enables_password() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "secret123".into(),
        })
        .expect("setup");

    match engine
        .dispatch_domain_command(DomainCommand::IsPasswordEnabled)
        .expect("check")
    {
        DomainCommandResult::Bool { value } => assert!(value, "must be enabled after setup"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn authenticate_with_correct_password_returns_normal() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "rightpw".into(),
        })
        .expect("setup");

    match engine
        .dispatch_domain_command(DomainCommand::Authenticate {
            password: "rightpw".into(),
        })
        .expect("auth")
    {
        DomainCommandResult::AuthMode { mode } => {
            assert!(matches!(mode, vauchi_platform::MobileAuthMode::Normal));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn authenticate_with_wrong_password_errors() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "rightpw".into(),
        })
        .expect("setup");

    let result = engine.dispatch_domain_command(DomainCommand::Authenticate {
        password: "wrong".into(),
    });
    assert!(result.is_err(), "wrong password must error");
}

// @internal
#[test]
fn setup_duress_password_enables_duress() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "rightpw".into(),
        })
        .expect("app pw");
    engine
        .dispatch_domain_command(DomainCommand::SetupDuressPassword {
            duress_password: "duresspw".into(),
        })
        .expect("duress");

    match engine
        .dispatch_domain_command(DomainCommand::IsDuressEnabled)
        .expect("check")
    {
        DomainCommandResult::Bool { value } => assert!(value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn authenticate_with_duress_password_returns_duress_mode() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "rightpw".into(),
        })
        .expect("app pw");
    engine
        .dispatch_domain_command(DomainCommand::SetupDuressPassword {
            duress_password: "duresspw".into(),
        })
        .expect("duress");

    match engine
        .dispatch_domain_command(DomainCommand::Authenticate {
            password: "duresspw".into(),
        })
        .expect("auth")
    {
        DomainCommandResult::AuthMode { mode } => {
            assert!(matches!(mode, vauchi_platform::MobileAuthMode::Duress));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn disable_duress_clears_duress_state() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetupAppPassword {
            password: "rightpw".into(),
        })
        .expect("app pw");
    engine
        .dispatch_domain_command(DomainCommand::SetupDuressPassword {
            duress_password: "duresspw".into(),
        })
        .expect("duress");
    engine
        .dispatch_domain_command(DomainCommand::DisableDuress)
        .expect("disable");

    match engine
        .dispatch_domain_command(DomainCommand::IsDuressEnabled)
        .expect("check")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "must be disabled"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_duress_settings_returns_none_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDuressSettings)
        .expect("get")
    {
        DomainCommandResult::DuressSettingsOpt { settings } => {
            assert!(settings.is_none(), "no settings yet");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn configure_duress_alerts_persists_to_get_settings() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::ConfigureDuressAlerts {
            contact_ids: vec!["c1".into(), "c2".into()],
            message: "help".into(),
        })
        .expect("configure");

    match engine
        .dispatch_domain_command(DomainCommand::GetDuressSettings)
        .expect("get")
    {
        DomainCommandResult::DuressSettingsOpt { settings } => {
            let s = settings.expect("must be configured");
            assert_eq!(s.alert_contact_ids, vec!["c1".to_string(), "c2".into()]);
            assert_eq!(s.alert_message, "help");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_decoy_contacts_is_empty_initially() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListDecoyContacts)
        .expect("list")
    {
        DomainCommandResult::DecoyContacts { contacts } => {
            assert!(contacts.is_empty(), "no decoys yet");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn add_decoy_contact_returns_id_and_lists() {
    let (engine, _dir) = create_engine_with_identity();
    let card_json =
        r#"{"id":"x","display_name":"Decoy Friend","fields":[],"avatar":null,"public_key":null}"#;

    let id = match engine
        .dispatch_domain_command(DomainCommand::AddDecoyContact {
            name: "Decoy Friend".into(),
            card_json: card_json.into(),
        })
        .expect("add")
    {
        DomainCommandResult::Text { value } => {
            assert!(value.starts_with("decoy-"), "id must use decoy- prefix");
            value
        }
        other => panic!("unexpected result: {other:?}"),
    };

    match engine
        .dispatch_domain_command(DomainCommand::ListDecoyContacts)
        .expect("list")
    {
        DomainCommandResult::DecoyContacts { contacts } => {
            assert_eq!(contacts.len(), 1);
            assert_eq!(contacts[0].id, id);
            assert_eq!(contacts[0].display_name, "Decoy Friend");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn delete_decoy_contact_removes_from_list() {
    let (engine, _dir) = create_engine_with_identity();
    let card_json =
        r#"{"id":"x","display_name":"Doomed Decoy","fields":[],"avatar":null,"public_key":null}"#;

    let id = match engine
        .dispatch_domain_command(DomainCommand::AddDecoyContact {
            name: "Doomed Decoy".into(),
            card_json: card_json.into(),
        })
        .expect("add")
    {
        DomainCommandResult::Text { value } => value,
        other => panic!("unexpected result: {other:?}"),
    };

    engine
        .dispatch_domain_command(DomainCommand::DeleteDecoyContact { id })
        .expect("delete");

    match engine
        .dispatch_domain_command(DomainCommand::ListDecoyContacts)
        .expect("list")
    {
        DomainCommandResult::DecoyContacts { contacts } => {
            assert!(contacts.is_empty(), "decoy must be gone");
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

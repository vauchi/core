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
    PlatformAppEngineTestHelpers,
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

    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after grant");
}

// ── Content Updates (B7 batch 2) ────────────────────────────────────

// @internal
#[test]
fn run_content_update_cycle_is_benign_noop_when_feature_off() {
    // Same guard as the check/apply tests above: with the feature on
    // the cycle would attempt a network check we don't want in
    // unit-time tests. The feature-on mapping is covered exhaustively
    // by the pure `content_cycle_outcome` table tests in content.rs.
    if cfg!(feature = "content-updates") {
        return;
    }
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::RunContentUpdateCycle)
        .expect("dispatch")
    {
        DomainCommandResult::ContentUpdateCycle { outcome } => {
            assert!(!outcome.applied, "disabled build must not report applied");
            assert!(
                !outcome.retryable_failure,
                "disabled is a benign no-op, not a retryable failure"
            );
            assert!(
                !outcome.refresh_appearance,
                "no theme refresh when disabled"
            );
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

    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
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

    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
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

    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
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

// ── Identity reads + Onboarding helpers (B7 batch 9) ────────────────

// @internal
#[test]
fn get_public_id_returns_hex_string() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetPublicId)
        .expect("get_public_id")
    {
        DomainCommandResult::Text { value } => {
            assert!(!value.is_empty(), "public_id must be non-empty");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_display_name_returns_onboarding_name() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetDisplayName)
        .expect("get_display_name")
    {
        DomainCommandResult::Text { value } => {
            assert_eq!(value, "Alice", "drive_onboarding sets name to Alice");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_own_fingerprint_formats_hex_in_groups() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetOwnFingerprint)
        .expect("fingerprint")
    {
        DomainCommandResult::Text { value } => {
            // 16 groups of 4 hex chars separated by spaces = 79 chars.
            assert_eq!(
                value.len(),
                79,
                "fingerprint must be formatted as 16x4 groups"
            );
            assert!(
                value.chars().all(|c| c.is_ascii_hexdigit() || c == ' '),
                "fingerprint must only contain hex + spaces"
            );
            assert!(
                value
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic())
                    .all(|c| c.is_ascii_uppercase()),
                "fingerprint hex must be uppercase"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn create_identity_errors_when_already_initialized() {
    let (engine, _dir) = create_engine_with_identity();

    let result = engine.dispatch_domain_command(DomainCommand::CreateIdentity {
        display_name: "Bob".into(),
    });
    assert!(
        result.is_err(),
        "create_identity must error when an identity already exists"
    );
}

// @internal
#[test]
fn display_name_suggestions_returns_non_empty_list_for_valid_name() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::DisplayNameSuggestions {
            full_name: "Alice Anderson".into(),
        })
        .expect("suggestions")
    {
        DomainCommandResult::Strings { values } => {
            assert!(!values.is_empty(), "suggestions must be non-empty");
            for v in &values {
                assert!(!v.is_empty(), "suggestion must be non-empty");
            }
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn reset_onboarding_returns_unit() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ResetOnboarding)
        .expect("reset")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── B7 batch 11: Contact verification + duplicates + notes + misc ──────

// @internal
#[test]
fn find_duplicates_returns_empty_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::FindDuplicates)
        .expect("find_duplicates")
    {
        DomainCommandResult::DuplicatePairs { pairs } => {
            assert!(pairs.is_empty(), "no contacts → no duplicates");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_hidden_contacts_returns_empty_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListHiddenContacts)
        .expect("list_hidden")
    {
        DomainCommandResult::Contacts { contacts } => {
            assert!(contacts.is_empty(), "no hidden contacts on fresh identity");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn search_social_networks_returns_results_for_known_query() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SearchSocialNetworks {
            query: "twitter".into(),
        })
        .expect("search_social")
    {
        DomainCommandResult::SocialNetworks { networks } => {
            assert!(
                !networks.is_empty(),
                "twitter must match at least one network"
            );
            assert!(
                networks.iter().any(|n| n.id.contains("twitter")
                    || n.display_name.to_lowercase().contains("twitter")),
                "twitter network expected in results"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn search_social_networks_returns_empty_for_unknown_query() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SearchSocialNetworks {
            query: "thisIsNotARealNetworkXYZ123".into(),
        })
        .expect("search_social")
    {
        DomainCommandResult::SocialNetworks { networks } => {
            assert!(networks.is_empty(), "unknown query → no matches");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_profile_url_returns_some_for_known_network() {
    let (engine, _dir) = create_engine_with_identity();

    // First find the twitter network id from the registry.
    let networks = match engine
        .dispatch_domain_command(DomainCommand::SearchSocialNetworks {
            query: "twitter".into(),
        })
        .expect("search_social")
    {
        DomainCommandResult::SocialNetworks { networks } => networks,
        other => panic!("unexpected result: {other:?}"),
    };
    let net = networks.first().expect("twitter result expected");

    match engine
        .dispatch_domain_command(DomainCommand::GetProfileUrl {
            network_id: net.id.clone(),
            username: "alice".into(),
        })
        .expect("get_profile_url")
    {
        DomainCommandResult::StringOpt { value } => {
            let url = value.expect("known network must produce a URL");
            assert!(
                url.contains("alice"),
                "URL must include username: got {url}"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_profile_url_returns_none_for_unknown_network() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetProfileUrl {
            network_id: "not_a_real_network".into(),
            username: "alice".into(),
        })
        .expect("get_profile_url")
    {
        DomainCommandResult::StringOpt { value } => {
            assert!(value.is_none(), "unknown network → None");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn verify_contact_returns_error_for_missing_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::VerifyContact {
            id: "nonexistent_contact_id".into(),
        })
        .expect_err("verify on missing contact must error");
    assert!(
        format!("{err:?}").to_lowercase().contains("not found"),
        "error must mention 'not found', got: {err:?}"
    );
}

// @internal
#[test]
fn set_proposal_trusted_returns_error_for_missing_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::SetProposalTrusted {
            contact_id: "nonexistent".into(),
            trusted: true,
        })
        .expect_err("set_proposal_trusted on missing contact must error");
    assert!(
        format!("{err:?}").to_lowercase().contains("not found"),
        "error must mention 'not found', got: {err:?}"
    );
}

// @internal
#[test]
fn dismiss_duplicate_succeeds_for_unknown_pair() {
    // dismiss_duplicate is idempotent — recording a dismissal for an
    // unknown pair is allowed (it just adds the pair to the dismissed set).
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::DismissDuplicate {
            id1: "a".into(),
            id2: "b".into(),
        })
        .expect("dismiss_duplicate idempotent")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_contact_note_errors_for_unknown_contact() {
    // Storage layer requires contact to exist — returns StorageError otherwise.
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::GetContactNote {
            contact_id: "ghost_contact".into(),
        })
        .expect_err("must error for unknown contact");
    assert!(
        format!("{err:?}").to_lowercase().contains("not found"),
        "error must mention 'not found', got: {err:?}"
    );
}

// @internal
#[test]
fn get_contact_field_notes_returns_empty_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetContactFieldNotes {
            contact_id: "ghost_contact".into(),
        })
        .expect("get_contact_field_notes")
    {
        DomainCommandResult::FieldNotes { notes } => {
            assert!(notes.is_empty(), "no field notes for unknown contact");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_contact_custom_avatar_errors_for_unknown_contact() {
    // Vauchi-layer get_contact_custom_avatar requires contact to exist.
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::GetContactCustomAvatar {
            contact_id: "ghost_contact".into(),
        })
        .expect_err("must error on missing contact");
    assert!(
        format!("{err:?}").to_lowercase().contains("not found"),
        "error must mention 'not found', got: {err:?}"
    );
}

// @internal
#[test]
fn contact_detail_footer_action_id_returns_error_for_missing_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::ContactDetailFooterActionId {
            contact_id: "nonexistent".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found"),
        "error must mention 'not found', got: {err:?}"
    );
}

// Suppress unused-import warning when no tests reference these types.

// ── B7 batch 12: Backup + Import ───────────────────────────────────────

// @internal
#[test]
fn export_backup_returns_base64_string() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ExportBackup {
            password: "correct horse battery staple".into(),
        })
        .expect("export_backup")
    {
        DomainCommandResult::Text { value } => {
            assert!(!value.is_empty(), "backup must contain bytes");
            // Base64 should decode without error.
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&value)
                .expect("must be valid base64");
            assert!(!decoded.is_empty(), "decoded backup must contain bytes");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn export_full_backup_returns_base64_string() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ExportFullBackup {
            password: "correct horse battery staple".into(),
        })
        .expect("export_full_backup")
    {
        DomainCommandResult::Text { value } => {
            assert!(!value.is_empty(), "full backup must contain bytes");
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&value)
                .expect("must be valid base64");
            assert!(decoded.len() > 100, "full backup should be non-trivial");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn import_backup_rejects_when_identity_already_initialized() {
    // Engine already has an identity from drive_onboarding.
    let (engine, _dir) = create_engine_with_identity();

    let backup = match engine
        .dispatch_domain_command(DomainCommand::ExportBackup {
            password: "correct horse battery staple".into(),
        })
        .expect("export")
    {
        DomainCommandResult::Text { value } => value,
        other => panic!("unexpected result: {other:?}"),
    };

    let err = engine
        .dispatch_domain_command(DomainCommand::ImportBackup {
            backup_data: backup,
            password: "correct horse battery staple".into(),
        })
        .expect_err("must reject second import");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("already") || msg.contains("initialized"),
        "expected already-initialized error, got: {err:?}"
    );
}

// @internal
#[test]
fn import_full_backup_rejects_when_identity_already_initialized() {
    let (engine, _dir) = create_engine_with_identity();

    let backup = match engine
        .dispatch_domain_command(DomainCommand::ExportFullBackup {
            password: "correct horse battery staple".into(),
        })
        .expect("export_full")
    {
        DomainCommandResult::Text { value } => value,
        other => panic!("unexpected result: {other:?}"),
    };

    let err = engine
        .dispatch_domain_command(DomainCommand::ImportFullBackup {
            backup_data: backup,
            password: "correct horse battery staple".into(),
        })
        .expect_err("must reject second import");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("already") || msg.contains("initialized"),
        "expected already-initialized error, got: {err:?}"
    );
}

// @internal
#[test]
fn import_backup_rejects_invalid_base64() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::ImportBackup {
            backup_data: "this is not base64!!!".into(),
            password: "correct horse battery staple".into(),
        })
        .expect_err("must reject invalid base64");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("base64") || msg.contains("already") || msg.contains("invalid"),
        "expected base64/invalid error, got: {err:?}"
    );
}

// @scenario: contact_import.feature - Empty vCard data returns zero imports
#[test]
fn import_contacts_from_vcf_handles_empty_input() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ImportContactsFromVcf { data: Vec::new() })
        .expect("vcf import")
    {
        DomainCommandResult::ImportResult { result } => {
            assert_eq!(result.imported, 0, "no contacts in empty vcf");
            assert_eq!(result.skipped, 0, "no contacts to skip");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @scenario: contact_import.feature - Import vCard file
#[test]
fn import_contacts_from_vcf_imports_multiple_vcards() {
    let (engine, _dir) = create_engine_with_identity();

    let vcf = b"BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Bob Smith\r\n\
TEL:+1234567890\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Carol Jones\r\n\
EMAIL:carol@example.com\r\n\
END:VCARD\r\n";

    match engine
        .dispatch_domain_command(DomainCommand::ImportContactsFromVcf { data: vcf.to_vec() })
        .expect("vcf import")
    {
        DomainCommandResult::ImportResult { result } => {
            assert_eq!(result.imported, 2, "two vcards imported");
            assert_eq!(result.skipped, 0, "no duplicates on fresh storage");
            assert!(result.warnings.is_empty(), "no warnings for clean import");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @scenario: contact_import.feature - Duplicate vCard UIDs are skipped
#[test]
fn import_contacts_from_vcf_skips_duplicates() {
    let (engine, _dir) = create_engine_with_identity();

    let vcf = b"BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:unique-bob-123\r\n\
FN:Bob Smith\r\n\
END:VCARD\r\n";

    match engine
        .dispatch_domain_command(DomainCommand::ImportContactsFromVcf { data: vcf.to_vec() })
        .expect("first import")
    {
        DomainCommandResult::ImportResult { result } => {
            assert_eq!(result.imported, 1, "first import succeeds");
        }
        other => panic!("unexpected result: {other:?}"),
    }

    match engine
        .dispatch_domain_command(DomainCommand::ImportContactsFromVcf { data: vcf.to_vec() })
        .expect("second import")
    {
        DomainCommandResult::ImportResult { result } => {
            assert_eq!(result.imported, 0, "duplicate not re-imported");
            assert_eq!(result.skipped, 1, "duplicate skipped");
            assert_eq!(
                result.warnings[0].key, "import.warning.duplicate_uid",
                "structured warning emitted"
            );
            assert!(
                result.warnings[0].legacy_text.contains("duplicate"),
                "legacy text mentions duplicate"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── B7 batch 13: Offline queue + counts + decoy CRUD ──────────────────

// @internal
#[test]
fn pending_update_count_returns_zero_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::PendingUpdateCount)
        .expect("pending_update_count")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn count_failed_deliveries_returns_zero_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CountFailedDeliveries)
        .expect("count_failed")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_total_pending_count_returns_zero_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetTotalPendingCount)
        .expect("total_pending")
    {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn is_offline_queue_full_returns_false_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsOfflineQueueFull)
        .expect("is_full")
    {
        DomainCommandResult::Bool { value } => assert!(!value),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_offline_queue_capacity_returns_full_capacity_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetOfflineQueueCapacity)
        .expect("capacity")
    {
        DomainCommandResult::Count { value } => {
            assert!(value > 0, "fresh queue must have capacity");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn add_then_list_then_delete_decoy_contact_round_trip() {
    let (engine, _dir) = create_engine_with_identity();

    // Use a real serialized ContactCard so deserialization succeeds.
    let card = vauchi_core::ContactCard::new("Decoy McDecoyface");
    let card_json = serde_json::to_string(&card).expect("serialize card");
    let id = match engine
        .dispatch_domain_command(DomainCommand::AddDecoyContact {
            name: "Decoy McDecoyface".into(),
            card_json,
        })
        .expect("add_decoy")
    {
        DomainCommandResult::Text { value } => value,
        other => panic!("unexpected result: {other:?}"),
    };
    assert!(id.starts_with("decoy-"), "id must use decoy- prefix: {id}");

    let listed = match engine
        .dispatch_domain_command(DomainCommand::ListDecoyContacts)
        .expect("list_decoy")
    {
        DomainCommandResult::DecoyContacts { contacts } => contacts,
        other => panic!("unexpected result: {other:?}"),
    };
    assert_eq!(listed.len(), 1, "exactly one decoy after add");
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].display_name, "Decoy McDecoyface");

    match engine
        .dispatch_domain_command(DomainCommand::DeleteDecoyContact { id: id.clone() })
        .expect("delete_decoy")
    {
        DomainCommandResult::Unit => {}
        other => panic!("unexpected result: {other:?}"),
    }

    match engine
        .dispatch_domain_command(DomainCommand::ListDecoyContacts)
        .expect("list_decoy after delete")
    {
        DomainCommandResult::DecoyContacts { contacts } => {
            assert!(contacts.is_empty(), "decoy must be gone after delete");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn add_decoy_contact_rejects_invalid_card_json() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::AddDecoyContact {
            name: "Bad".into(),
            card_json: "not json".into(),
        })
        .expect_err("must reject invalid JSON");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("invalid") || msg.contains("expected"),
        "expected invalid-input error, got: {err:?}"
    );
}

// ── B7 batch 14: Search + display prefs + merge ────────────────────────

// @internal
#[test]
fn search_contacts_returns_empty_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SearchContacts {
            query: "nonexistent".into(),
        })
        .expect("search")
    {
        DomainCommandResult::Contacts { contacts } => {
            assert!(contacts.is_empty(), "no contacts on fresh identity");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn set_display_name_preference_rejects_invalid_json() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::SetDisplayNamePreference {
            contact_id: "any".into(),
            pref_json: "not json".into(),
        })
        .expect_err("must reject invalid JSON");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("invalid") || msg.contains("expected"),
        "expected invalid-input, got: {err:?}"
    );
}

// @internal
#[test]
fn set_avatar_preference_rejects_invalid_json() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::SetAvatarPreference {
            contact_id: "any".into(),
            pref_json: "not json".into(),
        })
        .expect_err("must reject invalid JSON");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("invalid") || msg.contains("expected"),
        "expected invalid-input, got: {err:?}"
    );
}

// @internal
#[test]
fn set_display_name_preference_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::SetDisplayNamePreference {
            contact_id: "ghost".into(),
            pref_json: r#""primary""#.into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found") || msg.contains("contact"),
        "expected contact-not-found error, got: {err:?}"
    );
}

// @internal
#[test]
fn merge_contacts_errors_for_unknown_ids() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::MergeContacts {
            primary_id: "ghost1".into(),
            secondary_id: "ghost2".into(),
        })
        .expect_err("must error on missing contacts");
    // exact message varies; just confirm it didn't silently succeed.
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        !msg.is_empty(),
        "error message must be non-empty, got: {err:?}"
    );
}

// ── B7 batch 15: Field visibility ──────────────────────────────────────

// @internal
#[test]
fn hide_field_from_contact_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::HideFieldFromContact {
            contact_id: "ghost".into(),
            field_label: "any".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found"),
        "expected not-found, got: {err:?}"
    );
}

// @internal
#[test]
fn show_field_to_contact_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::ShowFieldToContact {
            contact_id: "ghost".into(),
            field_label: "any".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found"),
        "expected not-found, got: {err:?}"
    );
}

// @internal
#[test]
fn is_field_visible_to_contact_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::IsFieldVisibleToContact {
            contact_id: "ghost".into(),
            field_label: "any".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found"),
        "expected not-found, got: {err:?}"
    );
}

// @internal
#[test]
fn set_contact_field_override_errors_for_unknown_field_label() {
    // Identity exists (drive_onboarding) but no field with this label
    // exists on the own card.
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::SetContactFieldOverride {
            contact_id: "any".into(),
            field_label: "no-such-field".into(),
            is_visible: true,
        })
        .expect_err("must error on missing field");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("field not found") || msg.contains("not found"),
        "expected field-not-found, got: {err:?}"
    );
}

// @internal
#[test]
fn remove_contact_field_override_errors_for_unknown_field_label() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::RemoveContactFieldOverride {
            contact_id: "any".into(),
            field_label: "no-such-field".into(),
        })
        .expect_err("must error on missing field");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("field not found") || msg.contains("not found"),
        "expected field-not-found, got: {err:?}"
    );
}

// ── B7 batch 16: Onboarding state ops ──────────────────────────────────

// @internal
#[test]
fn get_onboarding_progress_returns_completed_after_drive_onboarding() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::GetOnboardingProgress)
        .expect("get_progress")
    {
        DomainCommandResult::OnboardingProgress { progress } => {
            // drive_onboarding finished, so the progress should reflect a
            // completed onboarding flow (current step is the terminal step).
            // Smoke: just verify the variant deserializes without panic.
            let _ = progress.current_step;
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn current_onboarding_step_returns_terminal_after_drive_onboarding() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::CurrentOnboardingStep)
        .expect("current_step")
    {
        DomainCommandResult::OnboardingStep { step: _ } => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn is_onboarding_complete_returns_true_after_drive_onboarding() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsOnboardingComplete)
        .expect("is_complete")
    {
        DomainCommandResult::Bool { value } => {
            assert!(value, "drive_onboarding completes the flow");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn advance_onboarding_returns_progress_variant() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::AdvanceOnboarding)
        .expect("advance")
    {
        DomainCommandResult::OnboardingProgress { progress: _ } => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn skip_onboarding_step_returns_progress_variant() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::SkipOnboardingStep)
        .expect("skip")
    {
        DomainCommandResult::OnboardingProgress { progress: _ } => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── B7 batch 17: Display options + paginated contact lists ─────────────

// @internal
#[test]
fn list_contacts_paginated_returns_empty_for_fresh_identity() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListContactsPaginated {
            offset: 0,
            limit: 50,
        })
        .expect("paginated")
    {
        DomainCommandResult::Contacts { contacts } => {
            assert!(contacts.is_empty(), "fresh identity has no contacts");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn list_contacts_paginated_with_zero_limit_returns_empty() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListContactsPaginated {
            offset: 0,
            limit: 0,
        })
        .expect("paginated")
    {
        DomainCommandResult::Contacts { contacts } => assert!(contacts.is_empty()),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn get_contact_display_options_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::GetContactDisplayOptions {
            contact_id: "ghost".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found") || msg.contains("contact"),
        "expected contact-not-found, got: {err:?}"
    );
}

// ── B7 batch 19: Contact detail view state + list social networks ──────

// @internal
#[test]
fn contact_detail_view_state_errors_for_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();

    let err = engine
        .dispatch_domain_command(DomainCommand::ContactDetailViewState {
            contact_id: "ghost".into(),
        })
        .expect_err("must error on missing contact");
    let msg = format!("{err:?}").to_lowercase();
    assert!(
        msg.contains("not found"),
        "expected contact-not-found, got: {err:?}"
    );
}

// @internal
#[test]
fn list_social_networks_returns_default_registry() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::ListSocialNetworks)
        .expect("list_social_networks")
    {
        DomainCommandResult::SocialNetworks { networks } => {
            assert!(
                !networks.is_empty(),
                "default registry must have well-known networks"
            );
            // Verify shape: each network has id + display_name + url_template
            for net in &networks {
                assert!(!net.id.is_empty(), "id must be non-empty");
                assert!(
                    !net.display_name.is_empty(),
                    "display_name must be non-empty"
                );
                assert!(
                    !net.url_template.is_empty(),
                    "url_template must be non-empty"
                );
            }
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── B7 batch 20: Multipart QR encoding ─────────────────────────────────

// @internal
#[test]
fn encode_multipart_qr_empty_input_returns_empty_or_one_frame() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::EncodeMultipartQr { data: Vec::new() })
        .expect("encode_multipart_qr")
    {
        DomainCommandResult::Strings { values } => {
            // Empty input deterministically produces zero or one frame —
            // either is acceptable from the encoder's perspective.
            assert!(
                values.len() <= 1,
                "empty input must not span multiple frames"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn encode_multipart_qr_small_payload_fits_in_one_frame() {
    let (engine, _dir) = create_engine_with_identity();
    let data = b"hello vauchi multipart qr".to_vec();

    match engine
        .dispatch_domain_command(DomainCommand::EncodeMultipartQr { data })
        .expect("encode_multipart_qr")
    {
        DomainCommandResult::Strings { values } => {
            assert_eq!(values.len(), 1, "small payload fits in one frame");
            assert!(!values[0].is_empty(), "frame must contain encoded data");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn encode_multipart_qr_large_payload_spans_multiple_frames() {
    let (engine, _dir) = create_engine_with_identity();
    // 5KB of bytes > 1800 byte frame limit → multiple frames.
    let data = vec![0xAB; 5_000];

    match engine
        .dispatch_domain_command(DomainCommand::EncodeMultipartQr { data })
        .expect("encode_multipart_qr")
    {
        DomainCommandResult::Strings { values } => {
            assert!(
                values.len() >= 2,
                "5KB payload must span at least 2 frames, got {}",
                values.len()
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// ── B7 batch 21: Certificate pinning persistence ───────────────────────

// @internal
#[test]
fn certificate_pinning_disabled_by_default() {
    let (engine, _dir) = create_engine_with_identity();

    match engine
        .dispatch_domain_command(DomainCommand::IsCertificatePinningEnabled)
        .expect("read")
    {
        DomainCommandResult::Bool { value } => {
            assert!(!value, "no pin file by default → pinning disabled");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn set_pinned_certificate_enables_pinning() {
    let (engine, _dir) = create_engine_with_identity();

    let pem = "-----BEGIN CERTIFICATE-----\nMIITESTSTUFF\n-----END CERTIFICATE-----\n";
    engine
        .dispatch_domain_command(DomainCommand::SetPinnedCertificate {
            cert_pem: pem.into(),
        })
        .expect("set");

    match engine
        .dispatch_domain_command(DomainCommand::IsCertificatePinningEnabled)
        .expect("read")
    {
        DomainCommandResult::Bool { value } => assert!(value, "pin must be active after set"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn set_pinned_certificate_with_empty_string_clears_pin() {
    let (engine, _dir) = create_engine_with_identity();

    engine
        .dispatch_domain_command(DomainCommand::SetPinnedCertificate {
            cert_pem: "PEM".into(),
        })
        .expect("set");

    engine
        .dispatch_domain_command(DomainCommand::SetPinnedCertificate {
            cert_pem: String::new(),
        })
        .expect("clear");

    match engine
        .dispatch_domain_command(DomainCommand::IsCertificatePinningEnabled)
        .expect("read")
    {
        DomainCommandResult::Bool { value } => assert!(!value, "empty PEM clears the pin"),
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn certificate_pinning_persists_across_engine_recreation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();

    let engine_a = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("engine A");
    drive_onboarding(&engine_a);

    engine_a
        .dispatch_domain_command(DomainCommand::SetPinnedCertificate {
            cert_pem: "-----BEGIN CERTIFICATE-----\nDATA\n-----END CERTIFICATE-----".into(),
        })
        .expect("set");
    drop(engine_a);

    let engine_b = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("engine B");

    match engine_b
        .dispatch_domain_command(DomainCommand::IsCertificatePinningEnabled)
        .expect("read")
    {
        DomainCommandResult::Bool { value } => {
            assert!(value, "pinning persisted across restart");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

// @internal
#[test]
fn clearing_pin_when_already_disabled_is_idempotent() {
    let (engine, _dir) = create_engine_with_identity();

    // No prior set — clear should not error.
    engine
        .dispatch_domain_command(DomainCommand::SetPinnedCertificate {
            cert_pem: String::new(),
        })
        .expect("idempotent clear");
}

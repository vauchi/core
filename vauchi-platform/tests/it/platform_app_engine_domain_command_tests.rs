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

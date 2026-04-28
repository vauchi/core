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

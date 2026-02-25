// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for get_consent_status API
//! Trace: ADR-021 Tier 1 — get_consent_status

use vauchi_core::api::*;
use vauchi_core::*;

fn create_test_vauchi() -> Vauchi<MockTransport> {
    Vauchi::in_memory().unwrap()
}

// @scenario: privacy_compliance:View what I consented to
// @scenario: privacy_compliance:Consent collected on first launch
#[test]
fn test_get_consent_status_returns_granted_with_timestamp_after_grant() {
    let wb = create_test_vauchi();

    // Grant consent
    wb.grant_consent(ConsentType::DataProcessing).unwrap();

    let status = wb.get_consent_status(ConsentType::DataProcessing).unwrap();

    assert!(status.granted, "consent should be granted");
    assert!(
        status.last_changed_at.is_some(),
        "last_changed_at should be set after granting"
    );
    // Timestamp should be recent (within 10 seconds)
    let ts = status.last_changed_at.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        now - ts < 10,
        "timestamp should be within 10 seconds of now, got diff: {}",
        now - ts
    );
}

// @scenario: privacy_compliance:Consent collected on first launch
#[test]
fn test_get_consent_status_returns_not_granted_when_never_set() {
    let wb = create_test_vauchi();

    let status = wb.get_consent_status(ConsentType::Analytics).unwrap();

    assert!(
        !status.granted,
        "consent should not be granted if never set"
    );
    assert!(
        status.last_changed_at.is_none(),
        "last_changed_at should be None if never set"
    );
    assert!(
        status.policy_version.is_none(),
        "policy_version should be None if never set"
    );
}

// @scenario: privacy_compliance:Withdraw consent for telemetry
#[test]
fn test_get_consent_status_returns_not_granted_after_revoke() {
    let wb = create_test_vauchi();

    // Grant then revoke
    wb.grant_consent(ConsentType::ContactSharing).unwrap();
    wb.revoke_consent(ConsentType::ContactSharing).unwrap();

    let status = wb.get_consent_status(ConsentType::ContactSharing).unwrap();

    assert!(
        !status.granted,
        "consent should not be granted after revocation"
    );
    assert!(
        status.last_changed_at.is_some(),
        "last_changed_at should still be present after revocation"
    );
}

// @scenario: privacy_compliance:Consent records include policy version
// @scenario: privacy_compliance:Re-consent required for major changes
#[test]
fn test_get_consent_status_includes_policy_version_when_granted_with_version() {
    let wb = create_test_vauchi();

    // Use the consent manager directly with a policy version
    let manager = ConsentManager::new(wb.storage());
    manager
        .grant_with_version(ConsentType::DataProcessing, "v2.1")
        .unwrap();

    let status = wb.get_consent_status(ConsentType::DataProcessing).unwrap();

    assert!(status.granted);
    assert_eq!(
        status.policy_version,
        Some("v2.1".to_string()),
        "policy_version should reflect the version set during grant"
    );
}

// @scenario: privacy_compliance:Consent for optional features
#[test]
fn test_get_consent_status_returns_different_statuses_for_different_types() {
    let wb = create_test_vauchi();

    wb.grant_consent(ConsentType::DataProcessing).unwrap();
    // Analytics left un-granted

    let dp_status = wb.get_consent_status(ConsentType::DataProcessing).unwrap();
    let analytics_status = wb.get_consent_status(ConsentType::Analytics).unwrap();

    assert!(dp_status.granted, "DataProcessing should be granted");
    assert!(!analytics_status.granted, "Analytics should not be granted");
}

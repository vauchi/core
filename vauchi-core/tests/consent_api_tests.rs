// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for consent management via the Vauchi public API.
//!
//! Tests that grant_consent(), revoke_consent(), check_consent(), and
//! export_consent_log() are properly delegated from Vauchi to ConsentManager.
//!
//! The consent storage uses second-precision timestamps with rowid tiebreaker
//! for ordering, so tests don't need sleeps between operations.

use vauchi_core::api::{ConsentRecord, ConsentType};
use vauchi_core::network::MockTransport;
use vauchi_core::Vauchi;

fn create_test_vauchi() -> Vauchi<MockTransport> {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("TestUser").unwrap();
    wb
}

// @scenario: privacy_compliance:Consent collected on first launch
// @scenario: privacy_compliance:Consent for optional features
#[test]
fn test_consent_grant_and_check() {
    let wb = create_test_vauchi();

    // Not granted initially
    let granted = wb.check_consent(&ConsentType::Analytics).unwrap();
    assert!(!granted, "Consent should not be granted initially");

    // Grant consent
    wb.grant_consent(ConsentType::Analytics).unwrap();

    // Now granted
    let granted = wb.check_consent(&ConsentType::Analytics).unwrap();
    assert!(granted, "Consent should be granted after grant_consent()");
}

// @scenario: privacy_compliance:Withdraw consent for telemetry
#[test]
fn test_consent_revoke() {
    let wb = create_test_vauchi();

    // Grant then revoke
    wb.grant_consent(ConsentType::ContactSharing).unwrap();
    assert!(wb.check_consent(&ConsentType::ContactSharing).unwrap());

    wb.revoke_consent(ConsentType::ContactSharing).unwrap();
    assert!(
        !wb.check_consent(&ConsentType::ContactSharing).unwrap(),
        "Consent should be revoked after revoke_consent()"
    );
}

// @scenario: privacy_compliance:Consent for optional features
// @scenario: privacy_compliance:Withdraw consent for telemetry
#[test]
fn test_consent_multiple_types_independent() {
    let wb = create_test_vauchi();

    wb.grant_consent(ConsentType::Analytics).unwrap();
    wb.grant_consent(ConsentType::DataProcessing).unwrap();

    // Revoking one doesn't affect the other

    wb.revoke_consent(ConsentType::Analytics).unwrap();

    assert!(
        !wb.check_consent(&ConsentType::Analytics).unwrap(),
        "Analytics should be revoked"
    );
    assert!(
        wb.check_consent(&ConsentType::DataProcessing).unwrap(),
        "DataProcessing should still be granted"
    );
}

// @scenario: privacy_compliance:View what I consented to
#[test]
fn test_consent_export_log() {
    let wb = create_test_vauchi();

    // Empty log initially
    let log = wb.export_consent_log().unwrap();
    assert!(log.is_empty(), "Consent log should be empty initially");

    // Grant and revoke some consents
    wb.grant_consent(ConsentType::Analytics).unwrap();
    wb.grant_consent(ConsentType::ContactSharing).unwrap();

    wb.revoke_consent(ConsentType::Analytics).unwrap();

    // Log should have 3 entries
    let log = wb.export_consent_log().unwrap();
    assert_eq!(log.len(), 3, "Should have 3 consent log entries");

    // Verify all types are present
    let analytics_entries: Vec<&ConsentRecord> = log
        .iter()
        .filter(|r| r.consent_type == ConsentType::Analytics)
        .collect();
    assert_eq!(
        analytics_entries.len(),
        2,
        "Should have 2 analytics entries (grant + revoke)"
    );

    let sharing_entries: Vec<&ConsentRecord> = log
        .iter()
        .filter(|r| r.consent_type == ConsentType::ContactSharing)
        .collect();
    assert_eq!(
        sharing_entries.len(),
        1,
        "Should have 1 contact sharing entry"
    );
}

// @scenario: privacy_compliance:Consent for optional features
#[test]
fn test_consent_grant_idempotent() {
    let wb = create_test_vauchi();

    // Grant twice
    wb.grant_consent(ConsentType::RecoveryVouching).unwrap();
    wb.grant_consent(ConsentType::RecoveryVouching).unwrap();

    // Should still be granted
    assert!(wb.check_consent(&ConsentType::RecoveryVouching).unwrap());

    // Log has 2 entries (both grants)
    let log = wb.export_consent_log().unwrap();
    let vouching_entries: Vec<&ConsentRecord> = log
        .iter()
        .filter(|r| r.consent_type == ConsentType::RecoveryVouching)
        .collect();
    assert_eq!(
        vouching_entries.len(),
        2,
        "Both grant operations should be logged"
    );
}

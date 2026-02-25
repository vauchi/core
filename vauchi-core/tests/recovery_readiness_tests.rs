// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for get_recovery_readiness API
//! Trace: ADR-021 Tier 1 — get_recovery_readiness

use vauchi_core::api::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::*;

fn create_test_vauchi() -> Vauchi<MockTransport> {
    Vauchi::in_memory().unwrap()
}

fn add_contact_with_recovery_trust(wb: &Vauchi<MockTransport>, pk: [u8; 32], trusted: bool) {
    let card = ContactCard::new("Contact");
    let shared_key = SymmetricKey::generate();
    let mut contact = Contact::from_exchange(pk, card, shared_key);
    if trusted {
        contact.trust_for_recovery();
    }
    wb.add_contact(contact).unwrap();
}

// @scenario: contact_recovery:Warning when trusted contacts below threshold
#[test]
fn test_get_recovery_readiness_returns_not_ready_below_threshold() {
    let wb = create_test_vauchi();

    // Default threshold is 3, add only 1 trusted contact
    add_contact_with_recovery_trust(&wb, [1u8; 32], true);
    // Add a non-trusted contact (should not count)
    add_contact_with_recovery_trust(&wb, [2u8; 32], false);

    let readiness = wb.get_recovery_readiness().unwrap();

    assert_eq!(
        readiness.trusted_count, 1,
        "only 1 contact is recovery-trusted"
    );
    assert_eq!(readiness.threshold, 3, "default recovery threshold is 3");
    assert!(
        !readiness.is_ready,
        "should not be ready with 1 < 3 trusted contacts"
    );
    assert_eq!(readiness.shortfall, 2, "shortfall should be 3 - 1 = 2");
}

// @scenario: contact_recovery:Recovery succeeds with trusted vouchers only
// @scenario: contact_recovery:Mark contact as trusted for recovery
#[test]
fn test_get_recovery_readiness_returns_ready_at_threshold() {
    let wb = create_test_vauchi();

    // Default threshold is 3, add exactly 3 trusted contacts
    for i in 0u8..3 {
        let mut pk = [0u8; 32];
        pk[0] = i + 10;
        add_contact_with_recovery_trust(&wb, pk, true);
    }

    let readiness = wb.get_recovery_readiness().unwrap();

    assert_eq!(readiness.trusted_count, 3);
    assert_eq!(readiness.threshold, 3);
    assert!(
        readiness.is_ready,
        "should be ready when trusted_count == threshold"
    );
    assert_eq!(
        readiness.shortfall, 0,
        "shortfall should be 0 when at threshold"
    );
}

// @scenario: contact_recovery:Recovery succeeds with trusted vouchers only
// @scenario: contact_recovery:Mark contact as trusted for recovery
#[test]
fn test_get_recovery_readiness_returns_ready_above_threshold() {
    let wb = create_test_vauchi();

    // Add 5 trusted contacts (above default threshold of 3)
    for i in 0u8..5 {
        let mut pk = [0u8; 32];
        pk[0] = i + 20;
        add_contact_with_recovery_trust(&wb, pk, true);
    }

    let readiness = wb.get_recovery_readiness().unwrap();

    assert_eq!(readiness.trusted_count, 5);
    assert!(readiness.is_ready);
    assert_eq!(readiness.shortfall, 0);
}

// @scenario: contact_recovery:Warning when trusted contacts below threshold
#[test]
fn test_get_recovery_readiness_with_no_contacts() {
    let wb = create_test_vauchi();

    let readiness = wb.get_recovery_readiness().unwrap();

    assert_eq!(readiness.trusted_count, 0);
    assert_eq!(readiness.threshold, 3);
    assert!(!readiness.is_ready);
    assert_eq!(
        readiness.shortfall, 3,
        "shortfall should equal threshold when no trusted contacts"
    );
}

// @scenario: contact_recovery:New contacts are not recovery-trusted by default
// @scenario: contact_recovery:Warning when trusted contacts below threshold
#[test]
fn test_get_recovery_readiness_excludes_non_trusted_contacts() {
    let wb = create_test_vauchi();

    // Add 5 contacts, none trusted for recovery
    for i in 0u8..5 {
        let mut pk = [0u8; 32];
        pk[0] = i + 30;
        add_contact_with_recovery_trust(&wb, pk, false);
    }

    let readiness = wb.get_recovery_readiness().unwrap();

    assert_eq!(
        readiness.trusted_count, 0,
        "non-trusted contacts should not count"
    );
    assert!(!readiness.is_ready);
    assert_eq!(readiness.shortfall, 3);
}

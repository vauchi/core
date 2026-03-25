// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Warnings Tests
//!
//! Tests for guardian diversity warnings and revocation reminders.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::contact::Contact;
use vauchi_core::contact::warnings::{check_guardian_diversity, check_revocation_reminders};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{ExchangeTransport, ProximityConfidence};

fn test_key() -> SymmetricKey {
    SymmetricKey::generate()
}

fn make_guardian(name: &str, transport: ExchangeTransport) -> Contact {
    let mut contact = Contact::from_exchange_full(
        [42u8; 32],
        ContactCard::new(name),
        test_key(),
        ProximityConfidence::Unknown,
        transport,
    );
    contact.set_recovery_trusted(true).unwrap();
    contact
}

#[test]
fn test_guardian_diversity_no_guardians_returns_none() {
    let contacts: Vec<Contact> = vec![];
    assert!(check_guardian_diversity(&contacts).is_none());
}

#[test]
fn test_guardian_diversity_single_guardian_returns_none() {
    let contacts = vec![make_guardian("Alice", ExchangeTransport::Qr)];
    assert!(check_guardian_diversity(&contacts).is_none());
}

#[test]
fn test_guardian_diversity_all_same_transport_warns() {
    let contacts = vec![
        make_guardian("Alice", ExchangeTransport::Qr),
        make_guardian("Bob", ExchangeTransport::Qr),
        make_guardian("Carol", ExchangeTransport::Qr),
    ];
    let warning = check_guardian_diversity(&contacts);
    assert!(
        warning.is_some(),
        "All guardians via QR should trigger diversity warning"
    );
    let w = warning.unwrap();
    assert_eq!(w.single_transport, ExchangeTransport::Qr);
    assert_eq!(w.guardian_count, 3);
}

#[test]
fn test_guardian_diversity_mixed_transport_no_warning() {
    let contacts = vec![
        make_guardian("Alice", ExchangeTransport::Qr),
        make_guardian("Bob", ExchangeTransport::Nfc),
        make_guardian("Carol", ExchangeTransport::Qr),
    ];
    assert!(check_guardian_diversity(&contacts).is_none());
}

#[test]
fn test_revocation_reminder_no_recovered_contacts() {
    let contacts = vec![Contact::from_exchange_full(
        [42u8; 32],
        ContactCard::new("Alice"),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    )];
    let reminders = check_revocation_reminders(&contacts);
    assert!(reminders.is_empty());
}

#[test]
fn test_revocation_reminder_recovered_unverified() {
    let mut contact = Contact::from_exchange_full(
        [42u8; 32],
        ContactCard::new("Alice"),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    contact.accept_recovery([99u8; 32], test_key()).unwrap();

    let reminders = check_revocation_reminders(&[contact]);
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].contact_id, hex::encode([99u8; 32]));
}

#[test]
fn test_revocation_reminder_recovered_but_verified_is_ok() {
    let mut contact = Contact::from_exchange_full(
        [42u8; 32],
        ContactCard::new("Alice"),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    contact.accept_recovery([99u8; 32], test_key()).unwrap();
    contact.mark_fingerprint_verified().unwrap();

    let reminders = check_revocation_reminders(&[contact]);
    assert!(
        reminders.is_empty(),
        "Verified recovered contacts should not generate reminders"
    );
}

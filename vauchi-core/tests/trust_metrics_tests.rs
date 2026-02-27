// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trust Metrics Tests
//!
//! Tests for contact enrichment: transport persistence, recovery flag,
//! card freshness, and storage roundtrip.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{ExchangeTransport, ProximityConfidence};

/// Helper: create a test contact card.
fn test_card() -> ContactCard {
    ContactCard::new("Test User")
}

/// Helper: create a test symmetric key.
fn test_key() -> SymmetricKey {
    SymmetricKey::generate()
}

/// Helper: create a test public key.
fn test_public_key() -> [u8; 32] {
    [42u8; 32]
}

// ============================================================
// Task 1: ExchangeTransport serde
// ============================================================

#[test]
fn test_exchange_transport_serde_roundtrip_qr() {
    let transport = ExchangeTransport::Qr;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Qr\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Qr);
}

#[test]
fn test_exchange_transport_serde_roundtrip_nfc() {
    let transport = ExchangeTransport::Nfc;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Nfc\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Nfc);
}

#[test]
fn test_exchange_transport_serde_roundtrip_ble() {
    let transport = ExchangeTransport::Ble;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Ble\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Ble);
}

// ============================================================
// Task 2: Trust fields on Contact
// ============================================================

#[test]
fn test_contact_from_exchange_full_preserves_transport() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::High,
        ExchangeTransport::Nfc,
    );
    assert_eq!(contact.exchange_transport(), ExchangeTransport::Nfc);
}

#[test]
fn test_contact_from_exchange_full_preserves_proximity() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::High,
        ExchangeTransport::Qr,
    );
    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);
}

#[test]
fn test_contact_default_has_recovered_is_false() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    assert!(!contact.has_recovered());
}

#[test]
fn test_contact_default_card_updated_at_is_none() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    assert_eq!(contact.card_updated_at(), None);
}

#[test]
fn test_contact_from_exchange_defaults_to_qr_transport() {
    let contact = Contact::from_exchange(test_public_key(), test_card(), test_key());
    assert_eq!(contact.exchange_transport(), ExchangeTransport::Qr);
}

// ============================================================
// Task 4: accept_recovery sets has_recovered
// ============================================================

#[test]
fn test_accept_recovery_sets_has_recovered_flag() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    assert!(!contact.has_recovered());

    let new_key = [99u8; 32];
    contact.accept_recovery(new_key, test_key());

    assert!(
        contact.has_recovered(),
        "Recovery flag must be set after accept_recovery()"
    );
    assert!(
        !contact.is_fingerprint_verified(),
        "Fingerprint must be reset after recovery"
    );
}

#[test]
fn test_has_recovered_is_permanent() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );

    contact.accept_recovery([99u8; 32], test_key());
    assert!(contact.has_recovered());

    // Verify fingerprint can be re-verified without clearing recovery flag
    contact.mark_fingerprint_verified();
    assert!(contact.has_recovered(), "Recovery flag must never be reset");
    assert!(contact.is_fingerprint_verified());
}

// ============================================================
// Task 5: update_card sets card_updated_at
// ============================================================

#[test]
fn test_update_card_sets_card_updated_at() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );
    assert_eq!(contact.card_updated_at(), None);

    let new_card = ContactCard::new("Updated Name");
    contact.update_card(new_card);

    assert!(
        contact.card_updated_at().is_some(),
        "card_updated_at must be set after update_card()"
    );
    assert_eq!(contact.display_name(), "Updated Name");
}

#[test]
fn test_update_card_timestamp_increases() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
    );

    contact.update_card(ContactCard::new("First Update"));
    let first_ts = contact.card_updated_at().unwrap();

    contact.update_card(ContactCard::new("Second Update"));
    let second_ts = contact.card_updated_at().unwrap();

    assert!(
        second_ts >= first_ts,
        "Second update timestamp must be >= first"
    );
}

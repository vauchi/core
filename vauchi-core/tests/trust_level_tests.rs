// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TrustLevel computation tests
//!
//! Verifies that `Contact::trust_level()` derives Verified/High/Standard/Cautious
//! from cryptographic exchange facts alone — no user-editable input.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::contact::Contact;
use vauchi_core::contact::trust::TrustLevel;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{ExchangeTransport, ProximityConfidence};

/// Build a base contact and apply a mutation for test setup.
fn make_contact(mutate: impl FnOnce(&mut Contact)) -> Contact {
    let public_key = [42u8; 32];
    let card = ContactCard::new("Test User");
    let shared_key = SymmetricKey::generate();
    let mut c = Contact::from_exchange(public_key, card, shared_key);
    mutate(&mut c);
    c
}

// ============================================================
// Cautious: has_recovered overrides everything
// ============================================================

#[test]
fn test_recovered_contact_is_cautious() {
    let contact = make_contact(|c| c.set_has_recovered(true));
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

#[test]
fn test_recovered_overrides_verified() {
    let contact = make_contact(|c| {
        c.set_has_recovered(true);
        c.mark_fingerprint_verified();
    });
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

#[test]
fn test_recovered_overrides_high_proximity_nfc() {
    let contact = make_contact(|c| {
        c.set_has_recovered(true);
        c.set_proximity_confidence(ProximityConfidence::High);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

// ============================================================
// Verified: fingerprint manually confirmed
// ============================================================

#[test]
fn test_fingerprint_verified_is_verified() {
    let contact = make_contact(|c| c.mark_fingerprint_verified());
    assert_eq!(contact.trust_level(), TrustLevel::Verified);
}

// ============================================================
// High: close-range transport with high proximity confidence
// ============================================================

#[test]
fn test_high_proximity_nfc_is_high() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::High);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

#[test]
fn test_high_proximity_ble_is_high() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::High);
        c.set_exchange_transport(ExchangeTransport::Ble);
    });
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

// ============================================================
// Standard: high proximity but QR (no close-range channel)
// ============================================================

#[test]
fn test_high_proximity_qr_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::High);
        c.set_exchange_transport(ExchangeTransport::Qr);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// ============================================================
// Standard: fallback for all other cases
// ============================================================

#[test]
fn test_default_contact_is_standard() {
    let contact = make_contact(|_| {});
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

#[test]
fn test_medium_proximity_nfc_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::Medium);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

#[test]
fn test_unknown_proximity_ble_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::Unknown);
        c.set_exchange_transport(ExchangeTransport::Ble);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

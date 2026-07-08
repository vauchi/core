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
    let mut c = Contact::from_exchange(public_key, card, shared_key, 0);
    mutate(&mut c);
    c
}

// ============================================================
// Cautious: has_recovered overrides everything
// ============================================================

// @internal
#[test]
fn test_recovered_contact_is_cautious() {
    let contact = make_contact(|c| c.set_has_recovered(true));
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

// @internal
#[test]
fn test_recovered_then_verified_restores_trust() {
    // Fingerprint re-verification is an in-person act that
    // clears has_recovered and restores Verified trust.
    let contact = make_contact(|c| {
        c.set_has_recovered(true);
        c.mark_fingerprint_verified().unwrap();
    });
    assert_eq!(contact.trust_level(), TrustLevel::Verified);
}

// @internal
#[test]
fn test_recovered_without_reverification_stays_cautious() {
    // Without re-verification, recovered stays Cautious.
    // set_has_recovered is the raw setter (sync/restore path),
    // not mark_fingerprint_verified (in-person path).
    let contact = make_contact(|c| {
        c.mark_fingerprint_verified().unwrap();
        c.set_has_recovered(true);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

// @internal
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

// @internal
#[test]
fn test_fingerprint_verified_is_verified() {
    let contact = make_contact(|c| {
        c.mark_fingerprint_verified().unwrap();
    });
    assert_eq!(contact.trust_level(), TrustLevel::Verified);
}

// ============================================================
// High: close-range transport with high proximity confidence
// ============================================================

// @internal
#[test]
fn test_high_proximity_nfc_is_high() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::High);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

// @internal
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

// @internal
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

// @internal
#[test]
fn test_default_contact_is_standard() {
    let contact = make_contact(|_| {});
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// @internal
#[test]
fn test_medium_proximity_nfc_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::Medium);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// @internal
#[test]
fn test_unknown_proximity_ble_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::Unknown);
        c.set_exchange_transport(ExchangeTransport::Ble);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// @internal
#[test]
fn test_low_proximity_nfc_is_standard() {
    let contact = make_contact(|c| {
        c.set_proximity_confidence(ProximityConfidence::Low);
        c.set_exchange_transport(ExchangeTransport::Nfc);
    });
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// ============================================================
// TrustMetrics path: new contacts with metrics use transport
// proximity and verifier confidence for trust derivation
// ============================================================

// @internal
#[test]
fn usb_transport_with_metrics_is_high_trust() {
    use vauchi_core::exchange::TrustMetrics;

    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Usb,
        ProximityConfidence::Unknown,
        1711324800,
    );
    contact.set_trust_metrics(Some(metrics));
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

// @internal
#[test]
fn qr_with_high_proximity_is_high_trust() {
    use vauchi_core::exchange::TrustMetrics;

    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(ExchangeTransport::Qr, ProximityConfidence::High, 1711324800);
    contact.set_trust_metrics(Some(metrics));
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

// @internal
#[test]
fn ble_with_medium_proximity_is_standard_trust() {
    use vauchi_core::exchange::TrustMetrics;

    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Ble,
        ProximityConfidence::Medium,
        1711324800,
    );
    contact.set_trust_metrics(Some(metrics));
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// @internal
#[test]
fn recovered_overrides_usb_physical() {
    use vauchi_core::exchange::TrustMetrics;

    let mut contact = make_contact(|_| {});
    contact.set_has_recovered(true);
    let metrics = TrustMetrics::new(
        ExchangeTransport::Usb,
        ProximityConfidence::Unknown,
        1711324800,
    );
    contact.set_trust_metrics(Some(metrics));
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);
}

// @internal
#[test]
fn trust_metrics_present_no_signal_gives_standard() {
    use vauchi_core::exchange::TrustMetrics;

    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Qr,
        ProximityConfidence::Unknown,
        1711324800,
    );
    contact.set_trust_metrics(Some(metrics));
    assert_eq!(contact.trust_level(), TrustLevel::Standard);
}

// @internal
#[test]
fn legacy_contact_without_metrics_uses_old_logic() {
    let mut contact = make_contact(|_| {});
    contact.set_exchange_transport(ExchangeTransport::Ble);
    contact.set_proximity_confidence(ProximityConfidence::High);
    assert_eq!(contact.trust_level(), TrustLevel::High);
}

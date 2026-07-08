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
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ExchangeTransport,
    MockProximityVerifier, ProximityConfidence, TransportProximity, TrustMetrics, X3DHKeyPair,
};
use vauchi_core::{Identity, Storage};

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

// @internal
#[test]
fn test_exchange_transport_serde_roundtrip_qr() {
    let transport = ExchangeTransport::Qr;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, r#""qr""#);
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Qr);
}

// @internal
#[test]
fn test_exchange_transport_serde_roundtrip_nfc() {
    let transport = ExchangeTransport::Nfc;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, r#""nfc""#);
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Nfc);
}

// @internal
#[test]
fn test_exchange_transport_serde_roundtrip_ble() {
    let transport = ExchangeTransport::Ble;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, r#""ble""#);
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Ble);
}

// ============================================================
// Task 4: Usb and Audio variants + serde backward compat
// ============================================================

// @internal
#[test]
fn exchange_transport_usb_serde_roundtrip() {
    let transport = ExchangeTransport::Usb;
    let json = serde_json::to_string(&transport).expect("serialize");
    assert_eq!(json, r#""usb""#);
    let deserialized: ExchangeTransport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized, ExchangeTransport::Usb);
}

// @internal
#[test]
fn exchange_transport_audio_serde_roundtrip() {
    let transport = ExchangeTransport::Audio;
    let json = serde_json::to_string(&transport).expect("serialize");
    assert_eq!(json, r#""audio""#);
    let deserialized: ExchangeTransport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized, ExchangeTransport::Audio);
}

// @internal
#[test]
fn exchange_transport_legacy_pascal_case_deserializes() {
    let qr: ExchangeTransport = serde_json::from_str(r#""Qr""#).expect("legacy Qr");
    assert_eq!(qr, ExchangeTransport::Qr);
    let nfc: ExchangeTransport = serde_json::from_str(r#""Nfc""#).expect("legacy Nfc");
    assert_eq!(nfc, ExchangeTransport::Nfc);
    let ble: ExchangeTransport = serde_json::from_str(r#""Ble""#).expect("legacy Ble");
    assert_eq!(ble, ExchangeTransport::Ble);
}

// ============================================================
// Task 2: Trust fields on Contact
// ============================================================

// @internal
#[test]
fn test_contact_from_exchange_full_preserves_transport() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::High,
        ExchangeTransport::Nfc,
        0,
    );
    assert_eq!(contact.exchange_transport(), Some(ExchangeTransport::Nfc));
}

// @internal
#[test]
fn test_contact_from_exchange_full_preserves_proximity() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::High,
        ExchangeTransport::Qr,
        0,
    );
    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);
}

// @internal
#[test]
fn test_contact_default_has_recovered_is_false() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    assert!(!contact.has_recovered());
}

// @internal
#[test]
fn test_contact_default_card_updated_at_is_none() {
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    assert_eq!(contact.card_updated_at(), None);
}

// @internal
#[test]
fn test_contact_from_exchange_defaults_to_qr_transport() {
    let contact = Contact::from_exchange(test_public_key(), test_card(), test_key(), 0);
    assert_eq!(contact.exchange_transport(), Some(ExchangeTransport::Qr));
}

// ============================================================
// Task 4: accept_recovery sets has_recovered
// ============================================================

// @internal
#[test]
fn test_accept_recovery_sets_has_recovered_flag() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    assert!(!contact.has_recovered());

    let new_key = [99u8; 32];
    contact.accept_recovery(new_key, test_key(), 0).unwrap();

    assert!(
        contact.has_recovered(),
        "Recovery flag must be set after accept_recovery()"
    );
    assert!(
        !contact.is_fingerprint_verified(),
        "Fingerprint must be reset after recovery"
    );
}

// @internal
#[test]
fn test_has_recovered_is_permanent() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );

    contact.accept_recovery([99u8; 32], test_key(), 0).unwrap();
    assert!(contact.has_recovered());

    // Fingerprint re-verification clears recovery flag — in-person
    // verification re-establishes trust, clearing the recovery state
    contact.mark_fingerprint_verified().unwrap();
    assert!(
        !contact.has_recovered(),
        "Fingerprint verification must clear recovery flag"
    );
    assert!(contact.is_fingerprint_verified());
}

// ============================================================
// Task 5: update_card sets card_updated_at
// ============================================================

// @internal
#[test]
fn test_update_card_sets_card_updated_at() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    assert_eq!(contact.card_updated_at(), None);

    let new_card = ContactCard::new("Updated Name");
    contact.update_card(new_card, 0);

    assert!(
        contact.card_updated_at().is_some(),
        "card_updated_at must be set after update_card()"
    );
    assert_eq!(contact.display_name(), "Updated Name");
}

// @internal
#[test]
fn test_update_card_timestamp_increases() {
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );

    contact.update_card(ContactCard::new("First Update"), 0);
    let first_ts = contact.card_updated_at().unwrap();

    contact.update_card(ContactCard::new("Second Update"), 0);
    let second_ts = contact.card_updated_at().unwrap();

    assert!(
        second_ts >= first_ts,
        "Second update timestamp must be >= first"
    );
}

// ============================================================
// Task 3: Exchange session wires transport to Contact
// ============================================================

/// Helper: run a full QR exchange ceremony and return the completed contact.
fn run_full_qr_exchange() -> Contact {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob", 0);

    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @internal
#[test]
fn test_qr_exchange_session_sets_qr_transport_on_contact() {
    let contact = run_full_qr_exchange();
    assert_eq!(
        contact.exchange_transport(),
        Some(ExchangeTransport::Qr),
        "QR exchange must produce contact with Qr transport"
    );
}

// @internal
#[test]
fn test_qr_exchange_contact_has_recovered_is_false() {
    let contact = run_full_qr_exchange();
    assert!(
        !contact.has_recovered(),
        "Fresh exchange contact must not be marked as recovered"
    );
}

// @internal
#[test]
fn test_qr_exchange_contact_card_updated_at_is_none() {
    let contact = run_full_qr_exchange();
    assert_eq!(
        contact.card_updated_at(),
        None,
        "Fresh exchange contact must have no card_updated_at"
    );
}

// ============================================================
// Task 6-7: Storage roundtrip for trust metric fields
// ============================================================

// @internal
#[test]
fn test_storage_roundtrip_preserves_exchange_transport() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::High,
        ExchangeTransport::Nfc,
        0,
    );
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    assert_eq!(
        loaded.exchange_transport(),
        Some(ExchangeTransport::Nfc),
        "Storage must preserve exchange_transport"
    );
}

// @internal
#[test]
fn test_storage_roundtrip_preserves_has_recovered() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    contact.accept_recovery([99u8; 32], test_key(), 0).unwrap();
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    assert!(
        loaded.has_recovered(),
        "Storage must preserve has_recovered flag"
    );
}

// @internal
#[test]
fn test_storage_roundtrip_preserves_card_updated_at() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = Contact::from_exchange_full(
        test_public_key(),
        test_card(),
        test_key(),
        ProximityConfidence::Unknown,
        ExchangeTransport::Qr,
        0,
    );
    contact.update_card(ContactCard::new("Updated"), 0);
    let id = contact.id().to_string();
    let expected_ts = contact.card_updated_at().unwrap();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    assert_eq!(
        loaded.card_updated_at(),
        Some(expected_ts),
        "Storage must preserve card_updated_at timestamp"
    );
}

// @internal
#[test]
fn test_storage_roundtrip_default_trust_fields() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = Contact::from_exchange(test_public_key(), test_card(), test_key(), 0);
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    assert_eq!(
        loaded.exchange_transport(),
        Some(ExchangeTransport::Qr),
        "Default transport must be Qr"
    );
    assert!(
        !loaded.has_recovered(),
        "Default has_recovered must be false"
    );
    assert_eq!(
        loaded.card_updated_at(),
        None,
        "Default card_updated_at must be None"
    );
}

// ============================================================
// Task 6: TrustMetrics on Contact + storage roundtrip
// ============================================================

/// Helper: create a test contact with optional mutation.
fn make_contact(mutate: impl FnOnce(&mut Contact)) -> Contact {
    let mut c = Contact::from_exchange(test_public_key(), test_card(), test_key(), 0);
    mutate(&mut c);
    c
}

// @internal
#[test]
fn contact_trust_metrics_defaults_to_none() {
    let contact = make_contact(|_| {});
    assert!(contact.trust_metrics().is_none());
}

// @internal
#[test]
fn contact_stores_trust_metrics() {
    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Ble,
        ProximityConfidence::High,
        1_711_324_800,
    );
    contact.set_trust_metrics(Some(metrics));

    let m = contact.trust_metrics().expect("should have metrics");
    assert_eq!(m.transport, ExchangeTransport::Ble);
    assert_eq!(m.transport_proximity, TransportProximity::Proximate);
}

// @internal
#[test]
fn contact_trust_metrics_can_be_cleared() {
    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Nfc,
        ProximityConfidence::High,
        1_711_324_800,
    );
    contact.set_trust_metrics(Some(metrics));
    assert!(contact.trust_metrics().is_some());

    contact.set_trust_metrics(None);
    assert!(contact.trust_metrics().is_none());
}

// @internal
#[test]
fn storage_roundtrip_preserves_trust_metrics() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Ble,
        ProximityConfidence::High,
        1_711_324_800,
    );
    contact.set_trust_metrics(Some(metrics));
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    let m = loaded.trust_metrics().expect("must survive roundtrip");
    assert_eq!(m.transport, ExchangeTransport::Ble);
    assert_eq!(m.proximity, ProximityConfidence::High);
    assert_eq!(m.transport_proximity, TransportProximity::Proximate);
    assert_eq!(m.timestamp, 1_711_324_800);
}

// @internal
#[test]
fn storage_roundtrip_preserves_none_trust_metrics() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_contact(|_| {});
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();

    assert!(
        loaded.trust_metrics().is_none(),
        "Legacy contacts must have None trust_metrics after roundtrip"
    );
}

// @internal
#[test]
fn list_contacts_preserves_trust_metrics() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = make_contact(|_| {});
    let metrics = TrustMetrics::new(
        ExchangeTransport::Nfc,
        ProximityConfidence::High,
        1_711_400_000,
    );
    contact.set_trust_metrics(Some(metrics));

    storage.contacts().save_contact(&contact).unwrap();
    let contacts = storage.contacts().list_contacts().unwrap();

    assert_eq!(contacts.len(), 1);
    let m = contacts[0].trust_metrics().expect("must survive list");
    assert_eq!(m.transport, ExchangeTransport::Nfc);
    assert_eq!(m.transport_proximity, TransportProximity::ContactRange);
}

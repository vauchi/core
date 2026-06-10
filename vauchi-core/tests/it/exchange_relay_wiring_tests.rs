// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for wiring QR v3 relay fields into Contact (Phase 1E).
//!
//! Validates that relay metadata from exchange QR codes flows
//! correctly into the resulting Contact struct.

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, ExchangeState, MockProximityVerifier};
use vauchi_core::identity::Identity;

/// Runs a full QR exchange between two identities and returns the resulting contacts.
fn run_qr_exchange(
    alice_relay_url: Option<String>,
    bob_relay_url: Option<String>,
) -> (vauchi_core::contact::Contact, vauchi_core::contact::Contact) {
    let alice_id = Identity::create("Alice", 0);
    let bob_id = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let mut alice_session = ExchangeSession::new_qr(
        alice_id,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_id,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    alice_session.set_our_relay_url(alice_relay_url);
    bob_session.set_our_relay_url(bob_relay_url);

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let alice_contact = match alice_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete state, got {:?}", other),
    };
    let bob_contact = match bob_session.state() {
        ExchangeState::Complete { contact } => *contact.clone(),
        other => panic!("Expected Complete state, got {:?}", other),
    };

    (alice_contact, bob_contact)
}

// ── Relay metadata flows through exchange ──────────────────────────

// @internal
#[test]
fn exchange_with_relay_metadata_populates_contact() {
    let (alice_contact, bob_contact) = run_qr_exchange(
        Some("https://alice-relay.com".to_string()),
        Some("https://bob-relay.com".to_string()),
    );

    // Alice's contact (Bob) should have Bob's relay
    assert_eq!(
        alice_contact.relay_url().unwrap(),
        "https://bob-relay.com",
        "Alice should learn Bob's relay from exchange"
    );

    // Bob's contact (Alice) should have Alice's relay
    assert_eq!(
        bob_contact.relay_url().unwrap(),
        "https://alice-relay.com",
        "Bob should learn Alice's relay from exchange"
    );
}

// @internal
#[test]
fn exchange_without_relay_metadata_leaves_contact_fields_empty() {
    let (alice_contact, bob_contact) = run_qr_exchange(None, None);

    assert!(alice_contact.relay_url().is_none());
    assert!(bob_contact.relay_url().is_none());
}

// @internal
#[test]
fn exchange_with_partial_relay_metadata() {
    let (alice_contact, bob_contact) =
        run_qr_exchange(Some("https://alice-relay.com".to_string()), None);

    // Alice's contact (Bob) has no relay
    assert!(alice_contact.relay_url().is_none());

    // Bob's contact (Alice) has Alice's relay
    assert_eq!(bob_contact.relay_url().unwrap(), "https://alice-relay.com");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session-level exchange tests for mobile platform bindings.
//!
//! Extracted from exchange.rs to keep source file under 1000 lines.
//! These tests use only the public MobileExchangeSession API.

use std::sync::Arc;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeSession, ManualConfirmationVerifier, VerifierChain, VerifierMethod,
};
use vauchi_core::identity::Identity;
use vauchi_platform::{
    MobileExchangeSession, MobileExchangeState, MobileProximityHandler, create_qr_exchange_manual,
    create_qr_exchange_proximity,
};

struct SuccessHandler;
impl MobileProximityHandler for SuccessHandler {
    fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
        String::new()
    }
}

// @scenario: contact_exchange:Generate exchange QR code
#[test]
fn test_session_generates_qr() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");

    let session = create_qr_exchange_manual(identity, card);

    assert!(matches!(session.state(), MobileExchangeState::Idle));

    let qr_data = session.generate_qr().unwrap();
    assert!(qr_data.starts_with("wb://"));

    assert!(matches!(
        session.state(),
        MobileExchangeState::DisplayingQr { .. }
    ));
}

// @scenario: contact_exchange:Mutual QR exchange with bidirectional scanning
#[test]
fn test_session_mutual_qr_flow() {
    let alice = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let alice_session = create_qr_exchange_manual(alice, alice_card);
    let alice_qr = alice_session.generate_qr().unwrap();

    let bob_session = create_qr_exchange_manual(bob, bob_card);
    let bob_qr = bob_session.generate_qr().unwrap();

    alice_session.process_qr(bob_qr).unwrap();
    bob_session.process_qr(alice_qr).unwrap();

    assert!(matches!(
        alice_session.state(),
        MobileExchangeState::PeerScanned
    ));
    assert!(matches!(
        bob_session.state(),
        MobileExchangeState::PeerScanned
    ));

    alice_session.they_scanned_our_qr().unwrap();
    bob_session.they_scanned_our_qr().unwrap();

    assert!(matches!(
        alice_session.state(),
        MobileExchangeState::AwaitingKeyAgreement
    ));

    alice_session.perform_key_agreement().unwrap();
    bob_session.perform_key_agreement().unwrap();

    assert!(matches!(
        alice_session.state(),
        MobileExchangeState::AwaitingCardExchange
    ));

    alice_session
        .complete_card_exchange("Bob".to_string())
        .unwrap();
    bob_session
        .complete_card_exchange("Alice".to_string())
        .unwrap();

    assert!(matches!(
        alice_session.state(),
        MobileExchangeState::Complete { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        MobileExchangeState::Complete { .. }
    ));
}

// @scenario: contact_exchange:Incomplete exchange recovery
#[test]
fn test_finalize_requires_complete_state() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");

    let session = create_qr_exchange_manual(identity, card);
    let result = session.extract_contact();
    assert!(result.is_err(), "expected error");
}

// @scenario: contact_exchange:Successful QR code exchange with proximity
#[test]
fn test_confirm_proximity_manual_session() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");

    let session = create_qr_exchange_manual(identity, card);
    session
        .confirm_proximity()
        .expect("confirm_proximity should succeed on manual session");
}

// @scenario: contact_exchange:Successful QR code exchange with proximity
#[test]
fn test_confirm_proximity_proximity_session() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");

    let session = create_qr_exchange_proximity(identity, card, Box::new(SuccessHandler));
    session
        .confirm_proximity()
        .expect("confirm_proximity should be a no-op on proximity session");
}

// @scenario: contact_exchange:Exchange timeout after interruption
#[test]
fn test_session_not_timed_out_initially() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");

    let session = create_qr_exchange_manual(identity, card);
    assert!(!session.is_timed_out());
}

// === Phase B: Event wiring tests ===

#[test]
fn test_verification_confidence_defaults_to_unknown() {
    use vauchi_platform::MobileProximityConfidence;

    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);

    assert_eq!(
        session.verification_confidence(),
        MobileProximityConfidence::Unknown
    );
}

#[test]
fn test_get_verification_events_empty_before_verification() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);

    assert!(session.get_verification_events().is_empty());
}

#[test]
fn test_verification_events_populated_after_key_agreement() {
    use vauchi_platform::{MobileProximityConfidence, MobileProximityVerifierEvent};

    let alice = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let alice_session = {
        let verifier = ManualConfirmationVerifier::new();
        verifier.confirm();
        let mut chain = VerifierChain::new();
        chain.add(VerifierMethod::ManualConfirmation, Box::new(verifier));
        let session = ExchangeSession::new_qr(alice, alice_card, chain);
        Arc::new(MobileExchangeSession::new(session, None))
    };
    let bob_session = {
        let verifier = ManualConfirmationVerifier::new();
        verifier.confirm();
        let mut chain = VerifierChain::new();
        chain.add(VerifierMethod::ManualConfirmation, Box::new(verifier));
        let session = ExchangeSession::new_qr(bob, bob_card, chain);
        Arc::new(MobileExchangeSession::new(session, None))
    };

    let alice_qr = alice_session.generate_qr().unwrap();
    let bob_qr = bob_session.generate_qr().unwrap();
    alice_session.process_qr(bob_qr).unwrap();
    bob_session.process_qr(alice_qr).unwrap();
    alice_session.they_scanned_our_qr().unwrap();
    bob_session.they_scanned_our_qr().unwrap();

    alice_session.perform_key_agreement().unwrap();
    bob_session.perform_key_agreement().unwrap();

    let alice_events = alice_session.get_verification_events();
    assert!(
        !alice_events.is_empty(),
        "Alice events should be populated after key agreement"
    );

    let alice_completed = alice_events.iter().find(|e| {
        matches!(
            e,
            MobileProximityVerifierEvent::Completed {
                confidence: MobileProximityConfidence::Medium,
                ..
            }
        )
    });
    assert!(
        alice_completed.is_some(),
        "Alice should have a Completed event with Medium confidence"
    );
}

#[test]
fn test_proximity_factory_wraps_in_chain() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_proximity(identity, card, Box::new(SuccessHandler));
    assert!(session.get_verification_events().is_empty());
}

#[test]
fn test_manual_factory_wraps_in_chain() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);
    assert!(session.get_verification_events().is_empty());
}

// === Debug log wiring tests ===

#[test]
fn test_debug_log_disabled_by_default() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);

    assert!(session.get_exchange_debug_jsonl().is_none());
    assert!(session.get_exchange_debug_markdown().is_none());
}

#[test]
fn test_enable_debug_log_captures_events() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);

    session.enable_debug_log();
    session.generate_qr().unwrap();

    let jsonl = session
        .get_exchange_debug_jsonl()
        .expect("log should exist");
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(jsonl.contains("session_started"));
    assert!(jsonl.contains("qr_generated"));
}

#[test]
fn test_debug_log_markdown_output() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let session = create_qr_exchange_manual(identity, card);

    session.enable_debug_log();
    session.generate_qr().unwrap();

    let md = session
        .get_exchange_debug_markdown()
        .expect("log should exist");
    assert!(md.contains("# Exchange Debug Log"));
    assert!(md.contains("2 events"));
    assert!(md.contains("SessionStarted"));
    assert!(md.contains("QrGenerated"));
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ProximityConfidence integration
//!
//! Verifies that proximity confidence is:
//! - Defined as an enum with High, Medium, Low, Unknown variants
//! - Stored on Contact with backward-compatible Unknown default
//! - Emitted via ExchangeEvent::ProximityCheckCompleted
//! - Set on the Contact during exchange completion
//! - Derivable from exchange session proximity check

use vauchi_core::exchange::*;
use vauchi_core::*;

// ===== ProximityConfidence enum tests =====

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_default_is_unknown() {
    let confidence = ProximityConfidence::default();
    assert_eq!(confidence, ProximityConfidence::Unknown);
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_variants_exist() {
    let high = ProximityConfidence::High;
    let medium = ProximityConfidence::Medium;
    let low = ProximityConfidence::Low;
    let unknown = ProximityConfidence::Unknown;

    assert_ne!(high, medium);
    assert_ne!(high, low);
    assert_ne!(high, unknown);
    assert_ne!(medium, low);
    assert_ne!(medium, unknown);
    assert_ne!(low, unknown);
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_copy() {
    let confidence = ProximityConfidence::High;
    let copied = confidence; // Copy trait
    assert_eq!(confidence, copied);
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_debug() {
    let confidence = ProximityConfidence::High;
    let debug_str = format!("{:?}", confidence);
    assert!(debug_str.contains("High"));
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_serde_roundtrip() {
    let variants = vec![
        ProximityConfidence::High,
        ProximityConfidence::Medium,
        ProximityConfidence::Low,
        ProximityConfidence::Unknown,
    ];

    for original in variants {
        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: ProximityConfidence = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, deserialized);
    }
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_deserialize_missing_defaults_to_unknown() {
    #[derive(serde::Deserialize)]
    struct TestWrapper {
        #[serde(default)]
        confidence: ProximityConfidence,
    }

    let json = "{}";
    let wrapper: TestWrapper = serde_json::from_str(json).expect("deserialize");
    assert_eq!(wrapper.confidence, ProximityConfidence::Unknown);
}

// ===== Contact integration tests =====

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_confidence_stored_on_contact() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let shared_key = crypto::SymmetricKey::generate();

    let contact = Contact::from_exchange(*identity.signing_public_key(), card, shared_key);

    assert_eq!(
        *contact.proximity_confidence(),
        ProximityConfidence::Unknown
    );
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_contact_with_proximity_confidence() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let shared_key = crypto::SymmetricKey::generate();

    let contact = Contact::from_exchange_with_proximity(
        *identity.signing_public_key(),
        card,
        shared_key,
        ProximityConfidence::High,
    );

    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_legacy_contact_has_unknown_confidence() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let shared_key = crypto::SymmetricKey::generate();
    let visibility_rules = contact::VisibilityRules::new();

    let contact = Contact::from_sync_data(
        *identity.signing_public_key(),
        card,
        shared_key,
        1000,
        false,
        visibility_rules,
    );

    assert_eq!(
        *contact.proximity_confidence(),
        ProximityConfidence::Unknown
    );
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_contact_set_proximity_confidence() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let shared_key = crypto::SymmetricKey::generate();

    let mut contact = Contact::from_exchange(*identity.signing_public_key(), card, shared_key);

    assert_eq!(
        *contact.proximity_confidence(),
        ProximityConfidence::Unknown
    );

    contact.set_proximity_confidence(ProximityConfidence::High);
    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);

    contact.set_proximity_confidence(ProximityConfidence::Low);
    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::Low);
}

// ===== ExchangeEvent tests =====

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_proximity_check_completed_event_exists() {
    let event = ExchangeEvent::ProximityCheckCompleted {
        confidence: ProximityConfidence::High,
    };

    let debug = format!("{:?}", event);
    assert!(debug.contains("ProximityCheckCompleted"));
    assert!(debug.contains("High"));
}

// ===== Exchange session integration tests =====

// @scenario: contact_exchange.feature:Successful QR code exchange with proximity
#[test]
fn test_exchange_with_proximity_sets_high_confidence() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Run proximity check -- mock succeeds, so should set High
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_exchange_without_proximity_sets_low_confidence() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::failure();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Run proximity check -- mock fails, so should set Low
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(*contact.proximity_confidence(), ProximityConfidence::Low);
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_exchange_proximity_timeout_sets_low_confidence() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::timeout();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Run proximity check -- mock times out, so should set Low
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(*contact.proximity_confidence(), ProximityConfidence::Low);
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Manual proximity confirmation
#[test]
fn test_manual_confirmation_sets_medium_confidence() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let manual_verifier = ManualConfirmationVerifier::new();
    manual_verifier.confirm();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, manual_verifier);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Run proximity check -- manual verifier confirmed, gets Medium confidence
    bob_session.run_proximity_check();

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(*contact.proximity_confidence(), ProximityConfidence::Medium);
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// ===== AU-3: Audio challenge storage tests =====

// @scenario: contact_exchange.feature:Proximity verification session binding
#[test]
fn test_session_stores_audio_challenge_from_qr() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);
    let expected_challenge = *alice_qr.audio_challenge();

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Before QR processing, no challenge stored
    assert_eq!(bob_session.their_audio_challenge(), None);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // AU-3: After QR processing, challenge should be stored
    assert_eq!(
        bob_session.their_audio_challenge(),
        Some(&expected_challenge)
    );
}

// @scenario: contact_exchange.feature:Proximity verification session binding
#[test]
fn test_stored_audio_challenge_is_not_zeros() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Challenge from QR is cryptographically random, must not be all zeros
    let stored = bob_session
        .their_audio_challenge()
        .expect("challenge should be stored after QR processing");
    assert_ne!(*stored, [0u8; 16]);
}

// ===== AU-1: Challenge used in proximity check tests =====

// @scenario: contact_exchange.feature:Proximity verification session binding
#[test]
fn test_proximity_check_uses_qr_challenge_not_zeros() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);
    let expected_challenge = *alice_qr.audio_challenge();

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Manually run proximity check
    bob_session.run_proximity_check();

    // AU-1: Verify the mock received the QR's challenge, not zeros
    let emitted = bob_session.proximity_verifier().emitted_challenges();
    assert!(
        !emitted.is_empty(),
        "proximity check should have emitted a challenge"
    );
    assert_eq!(emitted[0], expected_challenge);
    assert_ne!(
        emitted[0], [0u8; 16],
        "challenge must not be hardcoded zeros"
    );
}

// ===== AU-2: Auto-invoke proximity check tests =====

// @scenario: contact_exchange.feature:Proximity auto-verification during exchange
#[test]
fn test_key_agreement_auto_runs_proximity_check_high() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // AU-2: NO manual run_proximity_check() call!
    // Proximity check should have run automatically during key agreement.

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(
                *contact.proximity_confidence(),
                ProximityConfidence::High,
                "auto-invoked proximity check with successful mock should yield High"
            );
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Proximity auto-verification during exchange
#[test]
fn test_key_agreement_auto_runs_proximity_check_medium() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let manual_verifier = ManualConfirmationVerifier::new();
    manual_verifier.confirm();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, manual_verifier);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // AU-2: NO manual run_proximity_check() call!

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(
                *contact.proximity_confidence(),
                ProximityConfidence::Medium,
                "auto-invoked proximity check with manual verifier should yield Medium"
            );
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Proximity auto-verification during exchange
#[test]
fn test_key_agreement_auto_runs_proximity_check_low_on_failure() {
    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let bob_identity = Identity::create("Bob");

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::failure();
    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // AU-2: NO manual run_proximity_check() call!

    let alice_card = ContactCard::new("Alice");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    match bob_session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(
                *contact.proximity_confidence(),
                ProximityConfidence::Low,
                "auto-invoked proximity check with failing mock should yield Low"
            );
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}

// @scenario: contact_exchange.feature:Proximity verification prevents remote exchange
#[test]
fn test_complete_exchange_stores_proximity_confidence() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let shared_key = crypto::SymmetricKey::generate();

    let contact = Contact::from_exchange_with_proximity(
        *identity.signing_public_key(),
        card.clone(),
        shared_key,
        ProximityConfidence::High,
    );

    assert_eq!(*contact.proximity_confidence(), ProximityConfidence::High);

    let cloned = contact.clone();
    assert_eq!(*cloned.proximity_confidence(), ProximityConfidence::High);
}

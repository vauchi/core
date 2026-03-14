// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for VerifierChain — priority-ordered verifier orchestrator.
//!
//! Tries verifiers in priority order, emits events on each attempt,
//! uses the first success. If all fail, reports AllMethodsExhausted.

#![cfg(feature = "testing")]

use std::time::Duration;
use vauchi_core::exchange::verifier_chain::VerifierChain;
use vauchi_core::exchange::verifier_event::{ProximityVerifierEvent, VerifierMethod};
use vauchi_core::exchange::{
    ManualConfirmationVerifier, MockProximityVerifier, ProximityConfidence, ProximityVerifier,
};

// ===== Basic chain behavior =====

#[test]
fn single_successful_verifier_completes() {
    let verifier = MockProximityVerifier::success();
    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(verifier));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_completed());
    assert_eq!(log.final_confidence(), Some(ProximityConfidence::High));
}

#[test]
fn single_failing_verifier_exhausts() {
    let verifier = MockProximityVerifier::failure();
    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(verifier));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_exhausted());
    assert!(!log.is_completed());
    assert_eq!(log.final_confidence(), None);
}

// ===== Fallback behavior =====

#[test]
fn falls_back_to_second_verifier_when_first_fails() {
    let failing = MockProximityVerifier::failure();
    let succeeding = MockProximityVerifier::success();

    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(failing));
    chain.add(VerifierMethod::AmbientAudio, Box::new(succeeding));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_completed());

    let has_fallback = log.events().iter().any(|e| {
        matches!(
            e,
            ProximityVerifierEvent::FallingBack {
                failed_method: VerifierMethod::Ultrasonic,
                next_method: VerifierMethod::AmbientAudio,
            }
        )
    });
    assert!(has_fallback, "Should have a FallingBack event");

    let completed_method = log.events().iter().find_map(|e| match e {
        ProximityVerifierEvent::Completed { method, .. } => Some(*method),
        _ => None,
    });
    assert_eq!(completed_method, Some(VerifierMethod::AmbientAudio));
}

#[test]
fn all_verifiers_fail_produces_exhausted() {
    let fail1 = MockProximityVerifier::failure();
    let fail2 = MockProximityVerifier::failure();
    let fail3 = MockProximityVerifier::failure();

    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(fail1));
    chain.add(VerifierMethod::AmbientAudio, Box::new(fail2));
    chain.add(VerifierMethod::Accelerometer, Box::new(fail3));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_exhausted());
    assert!(!log.is_completed());

    assert!(matches!(
        log.last().unwrap(),
        ProximityVerifierEvent::AllMethodsExhausted
    ));
}

// ===== Event ordering =====

#[test]
fn events_in_correct_order_for_fallback() {
    let failing = MockProximityVerifier::failure();
    let succeeding = MockProximityVerifier::success();

    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(failing));
    chain.add(VerifierMethod::AmbientAudio, Box::new(succeeding));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    let events = log.events();
    assert!(
        events.len() >= 3,
        "Should have at least 3 events, got {}",
        events.len()
    );

    assert!(matches!(
        &events[0],
        ProximityVerifierEvent::InProgress {
            method: VerifierMethod::Ultrasonic,
            ..
        }
    ));

    assert!(matches!(
        &events[1],
        ProximityVerifierEvent::MethodFailed {
            method: VerifierMethod::Ultrasonic,
            ..
        }
    ));

    assert!(matches!(
        &events[2],
        ProximityVerifierEvent::FallingBack {
            failed_method: VerifierMethod::Ultrasonic,
            next_method: VerifierMethod::AmbientAudio,
        }
    ));
}

#[test]
fn first_success_stops_chain() {
    let success1 = MockProximityVerifier::success();
    let success2 = MockProximityVerifier::success();

    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(success1));
    chain.add(VerifierMethod::AmbientAudio, Box::new(success2));

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    let completed_method = log.events().iter().find_map(|e| match e {
        ProximityVerifierEvent::Completed { method, .. } => Some(*method),
        _ => None,
    });
    assert_eq!(completed_method, Some(VerifierMethod::Ultrasonic));

    let has_ambient = log.events().iter().any(|e| {
        matches!(
            e,
            ProximityVerifierEvent::InProgress {
                method: VerifierMethod::AmbientAudio,
                ..
            }
        )
    });
    assert!(!has_ambient, "Second verifier should not be attempted");
}

// ===== Empty chain =====

#[test]
fn empty_chain_produces_exhausted() {
    let chain = VerifierChain::new();
    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_exhausted());
    assert!(!log.is_completed());
}

// ===== Timeout verifier =====

#[test]
fn timeout_verifier_falls_back() {
    let timeout_verifier = MockProximityVerifier::timeout();
    let success_verifier = MockProximityVerifier::success();

    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::Ultrasonic, Box::new(timeout_verifier));
    chain.add(
        VerifierMethod::ManualConfirmation,
        Box::new(success_verifier),
    );

    let challenge = [1u8; 16];
    let timeout = Duration::from_secs(5);
    let log = chain.verify(&challenge, &challenge, timeout, true);

    assert!(log.is_completed());

    let has_fallback = log.events().iter().any(|e| {
        matches!(
            e,
            ProximityVerifierEvent::FallingBack {
                failed_method: VerifierMethod::Ultrasonic,
                next_method: VerifierMethod::ManualConfirmation,
            }
        )
    });
    assert!(has_fallback);
}

// ===== ProximityVerifier trait impl (W4) =====

#[test]
fn trait_impl_populates_last_event_log() {
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::success()),
    );

    // Before any verification, last_event_log is None
    assert!(chain.last_event_log().is_none());

    let emit = [1u8; 16];
    let listen = [2u8; 16];
    let timeout = Duration::from_secs(5);

    let result = chain.verify_proximity_two_way(&emit, &listen, timeout, true);
    assert!(result.is_ok());

    // After verification, last_event_log is populated
    let log = chain
        .last_event_log()
        .expect("log should be populated after verify_proximity_two_way");
    assert!(log.is_completed());
    assert_eq!(log.final_confidence(), Some(ProximityConfidence::High));
}

#[test]
fn trait_impl_populates_last_event_log_on_failure() {
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::failure()),
    );

    let emit = [1u8; 16];
    let listen = [2u8; 16];
    let timeout = Duration::from_secs(5);

    let result = chain.verify_proximity_two_way(&emit, &listen, timeout, true);
    assert!(result.is_err());

    let log = chain
        .last_event_log()
        .expect("log should be populated even on failure");
    assert!(log.is_exhausted());
    assert!(!log.is_completed());
}

#[test]
fn confidence_level_reflects_winner_not_maximum() {
    // Chain: High-confidence (fails) → Medium-confidence (succeeds)
    // confidence_level() must return Medium (the winner), not High (the max).
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::failure()),
    );
    chain.add(
        VerifierMethod::ManualConfirmation,
        Box::new(ManualConfirmationVerifier::pre_confirmed()),
    );

    let emit = [1u8; 16];
    let listen = [2u8; 16];
    let timeout = Duration::from_secs(5);

    let result = chain.verify_proximity_two_way(&emit, &listen, timeout, true);
    assert!(result.is_ok());

    // The winning verifier is ManualConfirmation (Medium), not Ultrasonic (High)
    assert_eq!(chain.confidence_level(), ProximityConfidence::Medium);
}

#[test]
fn confidence_level_returns_unknown_before_any_verification() {
    let chain = VerifierChain::new();
    assert_eq!(chain.confidence_level(), ProximityConfidence::Unknown);
}

#[test]
fn confidence_level_returns_unknown_after_all_fail() {
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::failure()),
    );

    let emit = [1u8; 16];
    let listen = [2u8; 16];
    let timeout = Duration::from_secs(5);

    let _ = chain.verify_proximity_two_way(&emit, &listen, timeout, true);

    // All verifiers failed — no winner, so confidence is Unknown
    assert_eq!(chain.confidence_level(), ProximityConfidence::Unknown);
}

#[test]
fn individual_methods_return_not_supported() {
    let chain = VerifierChain::new();

    assert!(matches!(
        chain.emit_challenge(&[0u8; 16]),
        Err(vauchi_core::exchange::ProximityError::NotSupported)
    ));
    assert!(matches!(
        chain.listen_for_response(Duration::from_secs(1)),
        Err(vauchi_core::exchange::ProximityError::NotSupported)
    ));
    assert!(!chain.verify_response(&[0u8; 16], &[0x01]));
}

#[test]
fn trait_impl_verification_event_log_delegates_to_last_event_log() {
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::success()),
    );

    // Before verification, trait method returns None
    assert!(chain.verification_event_log().is_none());

    let emit = [1u8; 16];
    let listen = [2u8; 16];
    let timeout = Duration::from_secs(5);
    chain
        .verify_proximity_two_way(&emit, &listen, timeout, true)
        .unwrap();

    // After verification, trait method returns the same as last_event_log()
    let via_trait = chain
        .verification_event_log()
        .expect("trait method should return log");
    let via_direct = chain
        .last_event_log()
        .expect("direct method should return log");
    assert_eq!(via_trait.events().len(), via_direct.events().len());
    assert!(via_trait.is_completed());
}

// ===== Atomicity consistency tests =====

#[test]
fn log_and_confidence_are_consistent_after_fallback() {
    // Fail → Succeed chain: confidence must match the winning verifier's
    // method in the event log. Guards against future regressions if someone
    // splits the single Mutex<VerificationResult> back into two fields.
    let mut chain = VerifierChain::new();
    chain.add(
        VerifierMethod::Ultrasonic,
        Box::new(MockProximityVerifier::failure()),
    );
    chain.add(
        VerifierMethod::ManualConfirmation,
        Box::new(ManualConfirmationVerifier::pre_confirmed()),
    );

    let emit = [0u8; 16];
    let listen = [1u8; 16];
    chain
        .verify_proximity_two_way(&emit, &listen, Duration::from_secs(5), true)
        .unwrap();

    // Read both fields — they must agree on ManualConfirmation / Medium
    let confidence = chain.confidence_level();
    let log = chain.last_event_log().expect("log should exist");
    assert_eq!(confidence, ProximityConfidence::Medium);
    assert!(log.is_completed());

    let completed_method = log.events().iter().find_map(|e| match e {
        ProximityVerifierEvent::Completed { method, .. } => Some(*method),
        _ => None,
    });
    assert_eq!(completed_method, Some(VerifierMethod::ManualConfirmation));
}

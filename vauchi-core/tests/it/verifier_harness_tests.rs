// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-device test harness for proximity verification.
//!
//! SimulatedPeer models a device with configurable verifier capabilities.
//! The scenario matrix covers 9 cases (A-I) from the spec.

#![cfg(feature = "testing")]

use vauchi_core::exchange::ProximityConfidence;
use vauchi_core::exchange::verifier_event::VerifierMethod;
use vauchi_core::exchange::verifier_harness::{PeerCapabilities, Scenario, SimulatedPeer};

// ===== SimulatedPeer construction =====

// @internal
#[test]
fn peer_with_all_capabilities() {
    let peer = SimulatedPeer::new(PeerCapabilities {
        ultrasonic: true,
        ambient_audio: true,
        accelerometer: true,
        manual_confirmation: true,
    });
    assert_eq!(peer.capabilities().available_methods().len(), 4);
}

// @internal
#[test]
fn peer_with_no_capabilities() {
    let peer = SimulatedPeer::new(PeerCapabilities {
        ultrasonic: false,
        ambient_audio: false,
        accelerometer: false,
        manual_confirmation: false,
    });
    assert!(peer.capabilities().available_methods().is_empty());
}

// @internal
#[test]
fn peer_with_mobile_capabilities() {
    let peer = SimulatedPeer::mobile();
    let methods = peer.capabilities().available_methods();
    // Mobile: ultrasonic, ambient_audio, accelerometer, manual_confirmation
    assert!(methods.contains(&VerifierMethod::Ultrasonic));
    assert!(methods.contains(&VerifierMethod::Accelerometer));
    assert!(methods.contains(&VerifierMethod::ManualConfirmation));
}

// @internal
#[test]
fn peer_with_desktop_capabilities() {
    let peer = SimulatedPeer::desktop();
    let methods = peer.capabilities().available_methods();
    // Desktop: ambient_audio, manual_confirmation (no accelerometer typically)
    assert!(methods.contains(&VerifierMethod::AmbientAudio));
    assert!(methods.contains(&VerifierMethod::ManualConfirmation));
    assert!(!methods.contains(&VerifierMethod::Accelerometer));
}

// ===== Scenario execution =====

/// Scenario A: Both mobile, co-located, all verifiers succeed.
/// Expected: First verifier succeeds (ultrasonic), High confidence.
// @internal
#[test]
fn scenario_a_both_mobile_co_located_all_succeed() {
    let scenario = Scenario::new(
        SimulatedPeer::mobile(),
        SimulatedPeer::mobile(),
        true, // co_located
    )
    .with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(outcome.is_success());
    assert_eq!(outcome.confidence, Some(ProximityConfidence::High));
    assert_eq!(outcome.method_used, Some(VerifierMethod::Ultrasonic));
}

/// Scenario B: Both mobile, co-located, ultrasonic fails, ambient audio succeeds.
// @internal
#[test]
fn scenario_b_ultrasonic_fails_ambient_succeeds() {
    let scenario = Scenario::new(SimulatedPeer::mobile(), SimulatedPeer::mobile(), true)
        .with_verifier_failing(VerifierMethod::Ultrasonic);

    let outcome = scenario.run();
    assert!(outcome.is_success());
    assert_eq!(outcome.method_used, Some(VerifierMethod::AmbientAudio));
}

/// Scenario C: Mobile + Desktop, co-located.
/// Common methods: ambient_audio, manual_confirmation.
// @internal
#[test]
fn scenario_c_mobile_desktop_co_located() {
    let scenario = Scenario::new(SimulatedPeer::mobile(), SimulatedPeer::desktop(), true)
        .with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(outcome.is_success());
    // Should use ambient_audio (highest priority common method)
    let method = outcome.method_used.unwrap();
    assert!(
        method == VerifierMethod::AmbientAudio || method == VerifierMethod::ManualConfirmation,
        "Mobile+Desktop should use a common method, got {:?}",
        method
    );
}

/// Scenario D: Both mobile, NOT co-located.
/// All proximity verifiers should fail, leaving only manual confirmation.
// @internal
#[test]
fn scenario_d_both_mobile_remote() {
    let scenario = Scenario::new(
        SimulatedPeer::mobile(),
        SimulatedPeer::mobile(),
        false, // not co-located
    );

    let outcome = scenario.run();
    // Should exhaust all proximity methods (they fail when remote)
    // Manual confirmation is the last resort
    assert!(
        outcome.confidence != Some(ProximityConfidence::High)
            || outcome.method_used == Some(VerifierMethod::ManualConfirmation),
        "Remote devices should not get High confidence from proximity methods"
    );
}

/// Scenario E: Both desktop, co-located.
// @internal
#[test]
fn scenario_e_both_desktop_co_located() {
    let scenario = Scenario::new(SimulatedPeer::desktop(), SimulatedPeer::desktop(), true)
        .with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(outcome.is_success());
}

/// Scenario F: No common methods (one peer has nothing).
// @internal
#[test]
fn scenario_f_no_common_methods() {
    let empty_peer = SimulatedPeer::new(PeerCapabilities {
        ultrasonic: false,
        ambient_audio: false,
        accelerometer: false,
        manual_confirmation: false,
    });
    let scenario = Scenario::new(SimulatedPeer::mobile(), empty_peer, true);

    let outcome = scenario.run();
    assert!(!outcome.is_success());
    assert_eq!(outcome.confidence, None);
}

/// Scenario G: All methods timeout.
// @internal
#[test]
fn scenario_g_all_methods_timeout() {
    let scenario = Scenario::new(SimulatedPeer::mobile(), SimulatedPeer::mobile(), true)
        .with_all_verifiers_timing_out();

    let outcome = scenario.run();
    assert!(!outcome.is_success());
}

/// Scenario H: Accelerometer-only devices (e.g., watches).
// @internal
#[test]
fn scenario_h_accelerometer_only() {
    let watch = SimulatedPeer::new(PeerCapabilities {
        ultrasonic: false,
        ambient_audio: false,
        accelerometer: true,
        manual_confirmation: false,
    });
    let scenario = Scenario::new(watch.clone(), watch, true).with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(outcome.is_success());
    assert_eq!(outcome.method_used, Some(VerifierMethod::Accelerometer));
}

/// Scenario I: Manual-only fallback.
// @internal
#[test]
fn scenario_i_manual_only() {
    let manual_only = SimulatedPeer::new(PeerCapabilities {
        ultrasonic: false,
        ambient_audio: false,
        accelerometer: false,
        manual_confirmation: true,
    });
    let scenario =
        Scenario::new(manual_only.clone(), manual_only, true).with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(outcome.is_success());
    assert_eq!(
        outcome.method_used,
        Some(VerifierMethod::ManualConfirmation)
    );
}

// ===== VerificationOutcome =====

// @internal
#[test]
fn outcome_has_event_log() {
    let scenario = Scenario::new(SimulatedPeer::mobile(), SimulatedPeer::mobile(), true)
        .with_all_verifiers_succeeding();

    let outcome = scenario.run();
    assert!(!outcome.events.is_empty(), "Outcome should have events");
}

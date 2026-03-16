// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AccelerometerVerifier — tap/shake cross-correlation.
//!
//! Both devices record accelerometer data during a shared physical action
//! (table tap, phone bump). Cross-correlation of the magnitude envelopes
//! proves co-location — same physical impulse = same acceleration pattern.

#![cfg(feature = "testing")]

use std::time::Duration;
use vauchi_core::exchange::accelerometer::{
    AccelerometerBackend, AccelerometerConfig, AccelerometerSample, AccelerometerVerifier,
    MockAccelerometerBackend,
};
use vauchi_core::exchange::{ProximityConfidence, ProximityError, ProximityVerifier};

// ===== AccelerometerSample =====

#[test]
fn sample_magnitude_for_unit_vector() {
    let s = AccelerometerSample::new(1.0, 0.0, 0.0);
    assert!((s.magnitude() - 1.0).abs() < 0.001);
}

#[test]
fn sample_magnitude_for_3d_vector() {
    let s = AccelerometerSample::new(3.0, 4.0, 0.0);
    assert!((s.magnitude() - 5.0).abs() < 0.001);
}

// ===== AccelerometerConfig defaults =====

#[test]
fn default_config_has_sane_values() {
    let config = AccelerometerConfig::default();
    assert_eq!(config.recording_duration_ms, 3000);
    assert_eq!(config.sample_rate_hz, 100);
    assert!(
        config.correlation_threshold > 0.3 && config.correlation_threshold < 1.0,
        "Threshold must be between 0.3 and 1.0, got {}",
        config.correlation_threshold
    );
}

// ===== Verifier trait compliance =====

#[test]
fn accelerometer_confidence_is_high() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    assert_eq!(verifier.confidence_level(), ProximityConfidence::High);
}

#[test]
fn accelerometer_emit_records_and_stores() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [42u8; 16];

    let result = verifier.emit_challenge(&challenge);
    result.expect("expected success");
}

#[test]
fn accelerometer_listen_returns_envelope() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [42u8; 16];

    verifier.emit_challenge(&challenge).unwrap();
    let response = verifier.listen_for_response(Duration::from_secs(5));
    response.expect("expected success");
    // Response is the serialized magnitude envelope (f32 per sample, 4 bytes each)
    let bytes = response.unwrap();
    assert!(
        bytes.len() > 0 && bytes.len() % 4 == 0,
        "Must be non-empty f32 array"
    );
}

#[test]
fn accelerometer_listen_fails_without_prior_emit() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));

    let result = verifier.listen_for_response(Duration::from_secs(5));
    assert!(matches!(result, Err(ProximityError::NoResponse)));
}

// ===== Co-location verification =====

#[test]
fn co_located_tap_passes_verification() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [1u8; 16];

    verifier.emit_challenge(&challenge).unwrap();
    let our_envelope = verifier
        .listen_for_response(Duration::from_secs(5))
        .unwrap();

    // Co-located mock produces same tap pattern
    assert!(
        verifier.verify_response(&challenge, &our_envelope),
        "Co-located devices with same tap should pass"
    );
}

#[test]
fn remote_device_fails_verification() {
    let backend = MockAccelerometerBackend::remote();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [1u8; 16];

    verifier.emit_challenge(&challenge).unwrap();

    // Remote device has completely different acceleration pattern
    let remote_envelope: Vec<u8> = (0..300)
        .flat_map(|i| {
            let val = (i as f32 * 0.7).sin() * 5.0;
            val.to_le_bytes().to_vec()
        })
        .collect();
    assert!(
        !verifier.verify_response(&challenge, &remote_envelope),
        "Remote device should fail verification"
    );
}

// ===== Backend unavailability =====

#[test]
fn unavailable_backend_returns_not_supported() {
    let backend = MockAccelerometerBackend::unavailable();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    let result = verifier.emit_challenge(&challenge);
    assert!(matches!(result, Err(ProximityError::NotSupported)));
}

// ===== Edge cases =====

#[test]
fn verify_response_rejects_wrong_length() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    verifier.emit_challenge(&challenge).unwrap();

    assert!(
        !verifier.verify_response(&challenge, &[]),
        "empty should be rejected"
    );
    assert!(
        !verifier.verify_response(&challenge, &[0u8; 3]),
        "non-multiple-of-4 should be rejected"
    );
}

#[test]
fn verify_response_rejects_without_local_data() {
    let backend = MockAccelerometerBackend::co_located();
    let verifier = AccelerometerVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    assert!(!verifier.verify_response(&challenge, &[0u8; 400]));
}

// ===== Cross-correlation math =====

#[test]
fn identical_envelopes_have_correlation_one() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    let signal: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
    let corr = cross_correlate(&signal, &signal);
    assert!(
        (corr - 1.0).abs() < 0.01,
        "Self-correlation should be ~1.0, got {}",
        corr
    );
}

#[test]
fn opposite_envelopes_have_negative_correlation() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    let signal: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
    let opposite: Vec<f32> = signal.iter().map(|&x| -x).collect();
    let corr = cross_correlate(&signal, &opposite);
    assert!(
        corr < -0.9,
        "Opposite signals should have correlation < -0.9, got {}",
        corr
    );
}

#[test]
fn uncorrelated_signals_near_zero() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    // Two signals with different frequencies
    let a: Vec<f32> = (0..300).map(|i| (i as f32 * 0.1).sin()).collect();
    let b: Vec<f32> = (0..300).map(|i| (i as f32 * 0.37).sin()).collect();
    let corr = cross_correlate(&a, &b);
    assert!(
        corr.abs() < 0.5,
        "Uncorrelated signals should have low correlation, got {}",
        corr
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AmbientAudioVerifier — passive room-noise fingerprinting.
//!
//! Based on SoundProof: both devices record ambient audio simultaneously,
//! compute SimHash fingerprints over mel-frequency bands, and compare.
//! If the Hamming distance between fingerprints is below a threshold,
//! devices are proven to be co-located.

#![cfg(feature = "testing")]

use std::time::Duration;
use vauchi_core::exchange::ambient_audio::{
    AmbientAudioBackend, AmbientAudioConfig, AmbientAudioVerifier, AudioFingerprint,
    MockAmbientAudioBackend,
};
use vauchi_core::exchange::{ProximityConfidence, ProximityError, ProximityVerifier};

// ===== AudioFingerprint core =====

#[test]
fn fingerprint_is_256_bits() {
    let fp = AudioFingerprint::zeros();
    assert_eq!(
        fp.as_bytes().len(),
        32,
        "Fingerprint must be 256 bits (32 bytes)"
    );
}

#[test]
fn identical_fingerprints_have_zero_hamming_distance() {
    let fp = AudioFingerprint::from_bytes([0xAB; 32]);
    assert_eq!(fp.hamming_distance(&fp), 0);
}

#[test]
fn opposite_fingerprints_have_max_hamming_distance() {
    let a = AudioFingerprint::from_bytes([0x00; 32]);
    let b = AudioFingerprint::from_bytes([0xFF; 32]);
    assert_eq!(a.hamming_distance(&b), 256);
}

#[test]
fn hamming_distance_is_symmetric() {
    let a = AudioFingerprint::from_bytes([0xAA; 32]);
    let b = AudioFingerprint::from_bytes([0x55; 32]);
    assert_eq!(a.hamming_distance(&b), b.hamming_distance(&a));
}

#[test]
fn similarity_of_identical_fingerprints_is_one() {
    let fp = AudioFingerprint::from_bytes([0xAB; 32]);
    let sim = fp.similarity(&fp);
    assert!((sim - 1.0).abs() < f32::EPSILON);
}

#[test]
fn similarity_of_opposite_fingerprints_is_zero() {
    let a = AudioFingerprint::from_bytes([0x00; 32]);
    let b = AudioFingerprint::from_bytes([0xFF; 32]);
    let sim = a.similarity(&b);
    assert!(sim.abs() < f32::EPSILON);
}

// ===== AmbientAudioConfig defaults =====

#[test]
fn default_config_has_sane_values() {
    let config = AmbientAudioConfig::default();
    assert_eq!(config.recording_duration_ms, 2000);
    assert_eq!(config.sample_rate, 16000);
    assert_eq!(config.num_mel_bands, 64);
    assert_eq!(config.fingerprint_bits, 256);
    assert!(
        config.similarity_threshold > 0.5 && config.similarity_threshold < 1.0,
        "Threshold must be between 0.5 and 1.0, got {}",
        config.similarity_threshold
    );
}

// ===== Verifier trait compliance =====

#[test]
fn ambient_audio_confidence_is_high() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    assert_eq!(verifier.confidence_level(), ProximityConfidence::High);
}

#[test]
fn ambient_audio_emit_records_and_stores_fingerprint() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [42u8; 16];

    // emit_challenge records ambient audio and stores local fingerprint
    let result = verifier.emit_challenge(&challenge);
    assert!(
        result.is_ok(),
        "emit_challenge should succeed for available backend"
    );
}

#[test]
fn ambient_audio_listen_returns_local_fingerprint() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [42u8; 16];

    // Must emit first to record and store fingerprint
    verifier.emit_challenge(&challenge).unwrap();

    // listen_for_response returns our local fingerprint (serialized)
    let response = verifier.listen_for_response(Duration::from_secs(5));
    response.expect("expected success");
    let fp_bytes = response.unwrap();
    assert_eq!(
        fp_bytes.len(),
        32,
        "Response must be 32 bytes (256-bit fingerprint)"
    );
}

#[test]
fn ambient_audio_listen_fails_without_prior_emit() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));

    // No emit_challenge called — no fingerprint stored
    let result = verifier.listen_for_response(Duration::from_secs(5));
    assert!(
        matches!(result, Err(ProximityError::NoResponse)),
        "Should fail without prior recording"
    );
}

// ===== Co-location verification =====

#[test]
fn co_located_devices_pass_verification() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [1u8; 16];

    verifier.emit_challenge(&challenge).unwrap();
    let our_fingerprint = verifier
        .listen_for_response(Duration::from_secs(5))
        .unwrap();

    // Peer's fingerprint is similar (co-located mock returns similar fingerprints)
    assert!(
        verifier.verify_response(&challenge, &our_fingerprint),
        "Co-located devices should pass verification"
    );
}

#[test]
fn remote_devices_fail_verification() {
    let backend = MockAmbientAudioBackend::remote();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [1u8; 16];

    verifier.emit_challenge(&challenge).unwrap();

    // Remote mock returns a completely different fingerprint
    let remote_fingerprint = [0xFF; 32].to_vec();
    assert!(
        !verifier.verify_response(&challenge, &remote_fingerprint),
        "Remote devices should fail verification"
    );
}

// ===== Backend unavailability =====

#[test]
fn unavailable_backend_returns_not_supported() {
    let backend = MockAmbientAudioBackend::unavailable();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    let result = verifier.emit_challenge(&challenge);
    assert!(
        matches!(result, Err(ProximityError::NotSupported)),
        "Unavailable backend should return NotSupported"
    );
}

// ===== verify_response edge cases =====

#[test]
fn verify_response_rejects_wrong_length() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    verifier.emit_challenge(&challenge).unwrap();

    assert!(
        !verifier.verify_response(&challenge, &[0u8; 16]),
        "16 bytes should be rejected"
    );
    assert!(
        !verifier.verify_response(&challenge, &[]),
        "empty should be rejected"
    );
    assert!(
        !verifier.verify_response(&challenge, &[0u8; 33]),
        "33 bytes should be rejected"
    );
}

#[test]
fn verify_response_rejects_without_local_fingerprint() {
    let backend = MockAmbientAudioBackend::co_located();
    let verifier = AmbientAudioVerifier::new(Box::new(backend));
    let challenge = [0u8; 16];

    // No emit_challenge called — no local fingerprint to compare against
    assert!(
        !verifier.verify_response(&challenge, &[0u8; 32]),
        "Should reject when no local fingerprint exists"
    );
}

// ===== SimHash computation (tested via fingerprint generation) =====

#[test]
fn same_samples_produce_same_fingerprint() {
    let samples: Vec<f32> = (0..32000).map(|i| (i as f32 * 0.01).sin()).collect();
    let config = AmbientAudioConfig::default();
    let fp1 = AudioFingerprint::compute(&samples, &config);
    let fp2 = AudioFingerprint::compute(&samples, &config);
    assert_eq!(fp1.hamming_distance(&fp2), 0);
}

#[test]
fn different_samples_produce_different_fingerprints() {
    let config = AmbientAudioConfig::default();

    // White noise vs sine wave should produce very different fingerprints
    let sine: Vec<f32> = (0..32000).map(|i| (i as f32 * 0.1).sin()).collect();
    // Deterministic "noise" pattern
    let noise: Vec<f32> = (0..32000)
        .map(|i| ((i as f32 * 7.3 + 0.5).sin() * 1000.0).fract())
        .collect();

    let fp_sine = AudioFingerprint::compute(&sine, &config);
    let fp_noise = AudioFingerprint::compute(&noise, &config);

    assert!(
        fp_sine.similarity(&fp_noise) < 0.8,
        "Different audio should produce dissimilar fingerprints, got similarity {}",
        fp_sine.similarity(&fp_noise)
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for mobile audio proximity verification (audio.rs).
//!
//! Covers: FSK encode/decode roundtrip, Goertzel frequency detection,
//! checksum encoding/verification, capability mapping, error cases.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vauchi_core::exchange::{AudioBackend, AudioCapability, AudioConfig};
use vauchi_platform::{MobileProximityVerifier, PlatformAudioBackend, PlatformAudioHandler};

/// Mock audio handler that captures emitted samples and returns them on receive.
/// This enables FSK encode→decode roundtrip testing without platform audio.
struct MockAudioHandler {
    capability: String,
    captured_samples: Mutex<Vec<f32>>,
    fail_emit: bool,
}

impl MockAudioHandler {
    fn full() -> Self {
        Self {
            capability: "full".to_string(),
            captured_samples: Mutex::new(Vec::new()),
            fail_emit: false,
        }
    }

    fn with_capability(cap: &str) -> Self {
        Self {
            capability: cap.to_string(),
            captured_samples: Mutex::new(Vec::new()),
            fail_emit: false,
        }
    }

    fn failing() -> Self {
        Self {
            capability: "full".to_string(),
            captured_samples: Mutex::new(Vec::new()),
            fail_emit: true,
        }
    }

    fn set_samples(&self, samples: Vec<f32>) {
        *self.captured_samples.lock().unwrap() = samples;
    }
}

impl PlatformAudioHandler for MockAudioHandler {
    fn check_capability(&self) -> String {
        self.capability.clone()
    }

    fn emit_signal(&self, samples: Vec<f32>, _sample_rate: u32) -> String {
        if self.fail_emit {
            return "Hardware error".to_string();
        }
        *self.captured_samples.lock().unwrap() = samples;
        String::new()
    }

    fn receive_signal(&self, _timeout_ms: u64, _sample_rate: u32) -> Vec<f32> {
        self.captured_samples.lock().unwrap().clone()
    }

    fn is_active(&self) -> bool {
        false
    }

    fn stop(&self) {}
}

// --- Capability mapping tests ---

#[test]
fn test_capability_mapping_full() {
    let handler = Arc::new(MockAudioHandler::with_capability("full"));
    let backend = PlatformAudioBackend::new(handler);
    assert_eq!(backend.check_capability(), AudioCapability::Full);
}

#[test]
fn test_capability_mapping_emit_only() {
    let handler = Arc::new(MockAudioHandler::with_capability("emit_only"));
    let backend = PlatformAudioBackend::new(handler);
    assert_eq!(backend.check_capability(), AudioCapability::EmitOnly);
}

#[test]
fn test_capability_mapping_receive_only() {
    let handler = Arc::new(MockAudioHandler::with_capability("receive_only"));
    let backend = PlatformAudioBackend::new(handler);
    assert_eq!(backend.check_capability(), AudioCapability::ReceiveOnly);
}

#[test]
fn test_capability_mapping_unknown() {
    let handler = Arc::new(MockAudioHandler::with_capability("something_else"));
    let backend = PlatformAudioBackend::new(handler);
    assert_eq!(backend.check_capability(), AudioCapability::None);
}

// --- Emit/receive capability enforcement tests ---

#[test]
fn test_emit_signal_not_supported_receive_only() {
    let handler = Arc::new(MockAudioHandler::with_capability("receive_only"));
    let backend = PlatformAudioBackend::new(handler);
    let config = AudioConfig::default();

    let result = backend.emit_signal(&[0x42], &config);
    assert!(result.is_err(), "ReceiveOnly should not emit");
    assert_eq!(
        format!("{:?}", result.unwrap_err()),
        "NotSupported",
        "Should return NotSupported error"
    );
}

#[test]
fn test_emit_signal_not_supported_none() {
    let handler = Arc::new(MockAudioHandler::with_capability("none"));
    let backend = PlatformAudioBackend::new(handler);
    let config = AudioConfig::default();

    let result = backend.emit_signal(&[0x42], &config);
    assert!(result.is_err(), "None capability should not emit");
}

#[test]
fn test_receive_signal_not_supported_emit_only() {
    let handler = Arc::new(MockAudioHandler::with_capability("emit_only"));
    let backend = PlatformAudioBackend::new(handler);
    let config = AudioConfig::default();

    let result = backend.receive_signal(Duration::from_millis(100), &config);
    assert!(result.is_err(), "EmitOnly should not receive");
    assert_eq!(
        format!("{:?}", result.unwrap_err()),
        "NotSupported",
        "Should return NotSupported error"
    );
}

#[test]
fn test_receive_signal_not_supported_none() {
    let handler = Arc::new(MockAudioHandler::with_capability("none"));
    let backend = PlatformAudioBackend::new(handler);
    let config = AudioConfig::default();

    let result = backend.receive_signal(Duration::from_millis(100), &config);
    assert!(result.is_err(), "None capability should not receive");
}

// --- FSK roundtrip tests (encode via emit → decode via receive) ---

#[test]
fn test_fsk_roundtrip_single_byte() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler.clone());
    let config = AudioConfig::default();

    // Emit encodes data as FSK samples and stores them via mock handler
    backend.emit_signal(&[0x42], &config).unwrap();

    // Receive reads back the stored samples and decodes FSK
    let decoded = backend
        .receive_signal(Duration::from_millis(1000), &config)
        .expect("Should decode FSK samples");

    assert!(
        !decoded.is_empty(),
        "Should decode at least 1 byte, got {}",
        decoded.len()
    );
    assert_eq!(decoded[0], 0x42, "First decoded byte should be 0x42");
}

#[test]
fn test_fsk_roundtrip_multi_byte() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler.clone());
    let config = AudioConfig::default();

    let data = [0x00, 0xFF, 0xA5];
    backend.emit_signal(&data, &config).unwrap();

    let decoded = backend
        .receive_signal(Duration::from_millis(1000), &config)
        .expect("Should decode multi-byte FSK");

    assert_eq!(
        decoded.len(),
        3,
        "Should decode exactly 3 bytes, got {}",
        decoded.len()
    );
    assert_eq!(decoded[0], 0x00, "First byte should be 0x00");
    assert_eq!(decoded[1], 0xFF, "Second byte should be 0xFF");
    assert_eq!(decoded[2], 0xA5, "Third byte should be 0xA5");
}

#[test]
fn test_fsk_roundtrip_challenge_16_bytes() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler.clone());
    let config = AudioConfig::default();

    let challenge: Vec<u8> = (0..16).collect();
    backend.emit_signal(&challenge, &config).unwrap();

    let decoded = backend
        .receive_signal(Duration::from_millis(1000), &config)
        .expect("Should decode 16-byte challenge");

    assert_eq!(
        decoded.len(),
        16,
        "Should decode exactly 16 bytes, got {}",
        decoded.len()
    );
    assert_eq!(
        decoded, challenge,
        "Decoded challenge should match original"
    );
}

// --- Empty/silence handling tests ---

#[test]
fn test_decode_empty_samples() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler.clone());
    let config = AudioConfig::default();

    // Set empty samples — receive should return Timeout (empty vec from handler)
    handler.set_samples(vec![]);
    let result = backend.receive_signal(Duration::from_millis(100), &config);
    assert!(result.is_err(), "Empty samples should return error");
}

#[test]
fn test_decode_silence_no_preamble() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler.clone());
    let config = AudioConfig::default();

    // Set silence — no preamble should be found
    let silence = vec![0.0f32; 44100]; // 1 second of silence
    handler.set_samples(silence);
    let result = backend.receive_signal(Duration::from_millis(1000), &config);
    assert!(result.is_err(), "Silence should not decode to data");
}

// --- Emit hardware error test ---

#[test]
fn test_emit_signal_hardware_error() {
    let handler = Arc::new(MockAudioHandler::failing());
    let backend = PlatformAudioBackend::new(handler);
    let config = AudioConfig::default();

    let result = backend.emit_signal(&[0x42], &config);
    assert!(result.is_err(), "Failing handler should return error");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("HardwareError"),
        "Should be HardwareError, got: {err}"
    );
}

// --- MobileProximityVerifier tests ---

#[test]
fn test_proximity_verifier_without_handler() {
    let verifier = MobileProximityVerifier::without_handler();
    assert!(
        !verifier.is_supported(),
        "Should not be supported without handler"
    );
    assert_eq!(
        verifier.get_capability(),
        "none",
        "Capability should be 'none' without handler"
    );
}

#[test]
fn test_proximity_verifier_with_full_handler() {
    let handler = Box::new(MockAudioHandler::full());
    let verifier = MobileProximityVerifier::new(handler);
    assert!(verifier.is_supported(), "Full handler should be supported");
    assert_eq!(
        verifier.get_capability(),
        "full",
        "Capability should be 'full'"
    );
}

#[test]
fn test_emit_challenge_checksum() {
    let handler = Box::new(MockAudioHandler::full());
    let verifier = MobileProximityVerifier::new(handler);

    let challenge = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let result = verifier.emit_challenge(challenge.clone());

    assert!(
        result.success,
        "Emit challenge should succeed: {}",
        result.error
    );
}

#[test]
fn test_emit_challenge_no_handler() {
    let verifier = MobileProximityVerifier::without_handler();
    let challenge = vec![1u8; 16];
    let result = verifier.emit_challenge(challenge);

    assert!(!result.success, "Should fail without handler");
    assert_eq!(
        result.error, "Audio handler not set",
        "Error should indicate no handler"
    );
}

#[test]
fn test_listen_for_response_valid_checksum() {
    let handler = MockAudioHandler::full();

    // Pre-build a valid encoded frame that the verifier would emit:
    // [length=17] [16 challenge bytes] [checksum = XOR of challenge bytes]
    let challenge_bytes: Vec<u8> = (1..=16).collect();
    let checksum: u8 = challenge_bytes.iter().fold(0u8, |acc, &b| acc ^ b);

    let mut encoded = Vec::with_capacity(18);
    encoded.push(17u8); // length
    encoded.extend(&challenge_bytes);
    encoded.push(checksum);

    // Generate FSK samples for this frame so the verifier can decode them
    // We'll emit them through a backend and capture, then set as receive data
    let emit_handler = Arc::new(MockAudioHandler::full());
    let emit_backend = PlatformAudioBackend::new(emit_handler.clone());
    let config = AudioConfig::default();
    emit_backend.emit_signal(&encoded, &config).unwrap();
    let samples = emit_handler.captured_samples.lock().unwrap().clone();

    // Set samples on the verifier's handler
    handler.set_samples(samples);
    let verifier = MobileProximityVerifier::new(Box::new(handler));

    let response = verifier.listen_for_response(1000);
    assert_eq!(response.len(), 16, "Should return 16 challenge bytes");
    assert_eq!(
        response, challenge_bytes,
        "Decoded challenge should match original"
    );
}

#[test]
fn test_listen_for_response_invalid_checksum() {
    let handler = MockAudioHandler::full();

    // Build a frame with an INVALID checksum
    let challenge_bytes: Vec<u8> = (1..=16).collect();
    let bad_checksum: u8 = 0xFF; // deliberately wrong

    let mut encoded = Vec::with_capacity(18);
    encoded.push(17u8);
    encoded.extend(&challenge_bytes);
    encoded.push(bad_checksum);

    let emit_handler = Arc::new(MockAudioHandler::full());
    let emit_backend = PlatformAudioBackend::new(emit_handler.clone());
    let config = AudioConfig::default();
    emit_backend.emit_signal(&encoded, &config).unwrap();
    let samples = emit_handler.captured_samples.lock().unwrap().clone();

    handler.set_samples(samples);
    let verifier = MobileProximityVerifier::new(Box::new(handler));

    let response = verifier.listen_for_response(1000);
    assert!(
        response.is_empty(),
        "Invalid checksum should return empty vec"
    );
}

#[test]
fn test_listen_for_response_no_handler() {
    let verifier = MobileProximityVerifier::without_handler();
    let response = verifier.listen_for_response(1000);
    assert!(
        response.is_empty(),
        "Should return empty vec without handler"
    );
}

#[test]
fn test_verifier_stop() {
    let handler = Box::new(MockAudioHandler::full());
    let verifier = MobileProximityVerifier::new(handler);
    // Should not panic
    verifier.stop();
    assert!(
        verifier.is_supported(),
        "Should still be supported after stop"
    );
}

#[test]
fn test_verifier_is_active() {
    let handler = Arc::new(MockAudioHandler::full());
    let backend = PlatformAudioBackend::new(handler);
    assert!(!backend.is_active(), "Mock should not be active initially");
}

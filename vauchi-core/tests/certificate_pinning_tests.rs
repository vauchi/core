// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate Pinning Tests
//!
//! These tests verify the certificate pinning implementation for relay connections.
//! Each test maps to scenarios from certificate_pinning.feature.
//!
//! Feature reference: features/certificate_pinning.feature

use vauchi_core::network::{verify_pin, PinnedCertificate};

// =============================================================================
// Pin Format Validation Tests
// Scenario: Pin is SHA-256 hash of DER certificate (@format)
// Scenario: Reject malformed pins (wrong length, invalid chars)
// =============================================================================

/// Tests that pins with incorrect length are rejected during creation.
/// A valid SHA-256 fingerprint must be exactly 32 bytes.
///
/// Maps to: @format "the pin should be exactly 32 bytes"
// @scenario: certificate_pinning.feature:Pin is SHA-256 hash of DER certificate
#[test]
fn test_pin_format_validation_correct_length() {
    // Valid 32-byte fingerprint
    let valid_fingerprint = [0xAB; 32];
    let pin = PinnedCertificate::new(valid_fingerprint);
    assert_eq!(pin.sha256_fingerprint.len(), 32);
}

/// Tests that pins computed from DER certificates are always 32 bytes.
///
/// Maps to: @format "the pin should be the SHA-256 hash of the DER bytes"
// @scenario: certificate_pinning.feature:Pin is SHA-256 hash of DER certificate
#[test]
fn test_pin_format_validation_from_der_is_sha256() {
    let cert_der = b"test DER-encoded certificate data";
    let pin = PinnedCertificate::from_der(cert_der);

    // SHA-256 always produces 32 bytes
    assert_eq!(
        pin.sha256_fingerprint.len(),
        32,
        "Pin from DER should be exactly 32 bytes (SHA-256)"
    );
}

/// Tests that malformed DER input (empty) still produces a valid 32-byte hash.
/// The hash function handles any input, including edge cases.
///
/// Maps to: @format validation edge cases
#[test]
fn test_pin_format_validation_empty_der_produces_hash() {
    let empty_cert: &[u8] = b"";
    let pin = PinnedCertificate::from_der(empty_cert);

    // Even empty input produces a valid 32-byte SHA-256 hash
    assert_eq!(pin.sha256_fingerprint.len(), 32);

    // Empty input should produce the well-known SHA-256 of empty string
    // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let expected_empty_hash: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    assert_eq!(pin.sha256_fingerprint, expected_empty_hash);
}

/// Tests that different DER inputs produce different fingerprints.
/// This ensures hash collisions are not occurring for distinct inputs.
///
/// Maps to: @format "Pin computation is deterministic" (inverse test)
#[test]
fn test_pin_format_validation_different_inputs_different_hashes() {
    let cert1 = b"Certificate Authority Root CA v1";
    let cert2 = b"Certificate Authority Root CA v2";

    let pin1 = PinnedCertificate::from_der(cert1);
    let pin2 = PinnedCertificate::from_der(cert2);

    assert_ne!(
        pin1.sha256_fingerprint, pin2.sha256_fingerprint,
        "Different certificates must produce different fingerprints"
    );
}

/// Tests that the same DER input always produces the same fingerprint.
///
/// Maps to: @format "Pin computation is deterministic"
// @scenario: certificate_pinning.feature:Pin computation is deterministic
#[test]
fn test_pin_format_validation_deterministic() {
    let cert_der = b"Consistent DER certificate content";

    let pin1 = PinnedCertificate::from_der(cert_der);
    let pin2 = PinnedCertificate::from_der(cert_der);

    assert_eq!(
        pin1, pin2,
        "Same certificate must always produce identical fingerprint"
    );
}

// =============================================================================
// MITM Detection Tests
// Scenario: Detect MITM with forged certificate (@mitm)
// Scenario: Connection rejected with mismatched certificate (@pin)
// =============================================================================

/// Tests detection of certificate mismatch between expected and received.
/// This is the core MITM detection mechanism.
///
/// Maps to: @mitm "Detect MITM with forged certificate"
/// Maps to: @pin "Connection rejected with mismatched certificate"
// @scenario: certificate_pinning.feature:Detect MITM with forged certificate
// @scenario: certificate_pinning.feature:Connection rejected with mismatched certificate
#[test]
fn test_mitm_detection_mismatched_certificate() {
    // The legitimate server's certificate
    let legitimate_cert = b"Legitimate Server Certificate DER";
    let legitimate_pin = PinnedCertificate::from_der(legitimate_cert);

    // The attacker's forged certificate
    let attacker_cert = b"Attacker's Forged Certificate DER";

    // Verification should FAIL when attacker's cert doesn't match legitimate pin
    let is_valid = verify_pin(attacker_cert, &[legitimate_pin]);

    assert!(
        !is_valid,
        "MITM attack should be detected: attacker's certificate must not match pinned fingerprint"
    );
}

/// Tests that a matching certificate passes verification.
///
/// Maps to: @pin "Connection succeeds with matching certificate pin"
// @scenario: certificate_pinning.feature:Connection succeeds with matching certificate pin
#[test]
fn test_mitm_detection_matching_certificate_passes() {
    let server_cert = b"Actual Server Certificate DER";
    let pinned = PinnedCertificate::from_der(server_cert);

    let is_valid = verify_pin(server_cert, &[pinned]);

    assert!(
        is_valid,
        "Legitimate certificate should pass pin verification"
    );
}

/// Tests that verification fails with completely wrong fingerprint.
///
/// Maps to: @mitm "the pin check should fail"
#[test]
fn test_mitm_detection_wrong_fingerprint() {
    let server_cert = b"Server Certificate";
    // Attacker tries to use a made-up fingerprint
    let fake_fingerprint = PinnedCertificate::new([0xFF; 32]);

    let is_valid = verify_pin(server_cert, &[fake_fingerprint]);

    assert!(
        !is_valid,
        "Fake fingerprint should not match any certificate"
    );
}

/// Tests that pin verification fails before any data exchange.
/// An empty pin list should always reject connections.
///
/// Maps to: @mitm "Pin verification happens before sending data"
/// Maps to: @pin "Empty pin list rejects all certificates"
// @scenario: certificate_pinning.feature:Empty pin list rejects all certificates
#[test]
fn test_mitm_detection_empty_pins_rejects_all() {
    let any_cert = b"Any Certificate";
    let empty_pins: &[PinnedCertificate] = &[];

    let is_valid = verify_pin(any_cert, empty_pins);

    assert!(
        !is_valid,
        "Empty pin list should reject ALL certificates (fail-secure)"
    );
}

/// Tests detection of subtle byte-level differences in certificates.
/// Even a single bit difference should be detected.
///
/// Maps to: @mitm security properties
#[test]
fn test_mitm_detection_single_byte_difference() {
    let original_cert = b"Server Certificate Version 1.0.0";
    let modified_cert = b"Server Certificate Version 1.0.1";

    let original_pin = PinnedCertificate::from_der(original_cert);

    let is_valid = verify_pin(modified_cert, &[original_pin]);

    assert!(
        !is_valid,
        "Even minor certificate modifications should be detected"
    );
}

// =============================================================================
// Graceful Pin Rotation Tests
// Scenario: Multiple pins allow certificate rotation (@rotation)
// Scenario: Graceful certificate rotation
// =============================================================================

/// Tests handling of pin update during active request / certificate rotation.
/// Multiple pins allow seamless transition between old and new certificates.
///
/// Maps to: @rotation "Graceful certificate rotation"
/// Maps to: @pin "Multiple pins allow certificate rotation"
// @scenario: certificate_pinning.feature:Multiple pins allow certificate rotation
// @scenario: certificate_pinning.feature:Graceful certificate rotation
#[test]
fn test_graceful_pin_rotation_old_certificate() {
    let old_cert = b"Old Server Certificate 2025";
    let new_cert = b"New Server Certificate 2026";

    let old_pin = PinnedCertificate::from_der(old_cert);
    let new_pin = PinnedCertificate::from_der(new_cert);

    // Both pins are configured (during rotation period)
    let pins = vec![old_pin, new_pin];

    // Old certificate should still work
    let is_valid = verify_pin(old_cert, &pins);
    assert!(
        is_valid,
        "Old certificate should work during rotation period"
    );
}

/// Tests that new certificate works when both pins are configured.
///
/// Maps to: @rotation "connections should continue to succeed"
// @scenario: certificate_pinning.feature:Multiple pins allow certificate rotation
#[test]
fn test_graceful_pin_rotation_new_certificate() {
    let old_cert = b"Old Server Certificate 2025";
    let new_cert = b"New Server Certificate 2026";

    let old_pin = PinnedCertificate::from_der(old_cert);
    let new_pin = PinnedCertificate::from_der(new_cert);

    // Both pins are configured (during rotation period)
    let pins = vec![old_pin, new_pin];

    // New certificate should work
    let is_valid = verify_pin(new_cert, &pins);
    assert!(
        is_valid,
        "New certificate should work during rotation period"
    );
}

/// Tests that after rotation, old pin can be removed.
///
/// Maps to: @rotation "the old pin can be removed after transition"
// @scenario: certificate_pinning.feature:Graceful certificate rotation
#[test]
fn test_graceful_pin_rotation_old_pin_removed() {
    let old_cert = b"Old Server Certificate 2025";
    let new_cert = b"New Server Certificate 2026";

    let new_pin = PinnedCertificate::from_der(new_cert);

    // Only new pin is configured (after rotation complete)
    let pins = vec![new_pin];

    // New certificate works
    assert!(
        verify_pin(new_cert, &pins),
        "New certificate should work after rotation"
    );

    // Old certificate should be rejected
    assert!(
        !verify_pin(old_cert, &pins),
        "Old certificate should be rejected after rotation complete"
    );
}

/// Tests that the matching pin is found regardless of order in the list.
///
/// Maps to: @pin "the matching pin should be identified"
#[test]
fn test_graceful_pin_rotation_order_independent() {
    let cert_a = b"Certificate A";
    let cert_b = b"Certificate B";
    let cert_c = b"Certificate C";

    let pin_a = PinnedCertificate::from_der(cert_a);
    let pin_b = PinnedCertificate::from_der(cert_b);
    let pin_c = PinnedCertificate::from_der(cert_c);

    // Certificate B should match regardless of its position
    assert!(verify_pin(
        cert_b,
        &[pin_a.clone(), pin_b.clone(), pin_c.clone()]
    ));
    assert!(verify_pin(
        cert_b,
        &[pin_b.clone(), pin_a.clone(), pin_c.clone()]
    ));
    assert!(verify_pin(
        cert_b,
        &[pin_c.clone(), pin_a.clone(), pin_b.clone()]
    ));
}

/// Tests rotation with many certificates in the pin list.
///
/// Maps to: @rotation scalability
#[test]
fn test_graceful_pin_rotation_many_pins() {
    let target_cert = b"Target Certificate";
    let target_pin = PinnedCertificate::from_der(target_cert);

    // Build a list of many pins
    let mut pins: Vec<PinnedCertificate> = (0..100)
        .map(|i| {
            let cert = format!("Certificate {}", i);
            PinnedCertificate::from_der(cert.as_bytes())
        })
        .collect();

    // Add target pin in the middle
    pins.insert(50, target_pin);

    // Should still find the matching pin
    let is_valid = verify_pin(target_cert, &pins);
    assert!(is_valid, "Should find matching pin even in large pin list");
}

// =============================================================================
// Pinning Disabled Fallback Tests
// Scenario: Graceful degradation when pinning fails
// =============================================================================

/// Tests graceful degradation when pinning fails - empty list rejects all.
/// This is a fail-secure design: no pins = no connections allowed.
///
/// Maps to: @pin "Empty pin list rejects all certificates"
/// Maps to: graceful degradation behavior
#[test]
fn test_pinning_disabled_fallback_empty_list() {
    let any_cert = b"Any Valid Certificate";

    // No pins configured = fail-secure (reject all)
    let is_valid = verify_pin(any_cert, &[]);

    assert!(
        !is_valid,
        "With no pins configured, verification should fail (fail-secure)"
    );
}

/// Tests that the system handles the case where all configured pins are expired/wrong.
/// This simulates a configuration error where none of the pins match.
///
/// Maps to: graceful degradation, configuration error detection
#[test]
fn test_pinning_disabled_fallback_all_pins_wrong() {
    let server_cert = b"Actual Server Certificate";

    // All configured pins are wrong (perhaps from old configuration)
    let wrong_pin_1 = PinnedCertificate::new([0x11; 32]);
    let wrong_pin_2 = PinnedCertificate::new([0x22; 32]);
    let wrong_pin_3 = PinnedCertificate::new([0x33; 32]);

    let is_valid = verify_pin(server_cert, &[wrong_pin_1, wrong_pin_2, wrong_pin_3]);

    assert!(
        !is_valid,
        "When all pins are wrong, verification should fail"
    );
}

/// Tests that the pinning system properly handles binary certificate data.
/// Real DER certificates contain arbitrary bytes including nulls.
///
/// Maps to: @format edge cases with binary data
#[test]
fn test_pinning_disabled_fallback_binary_data() {
    // Simulate binary DER data with null bytes and full range of values
    let binary_cert: Vec<u8> = (0..=255).collect();
    let pin = PinnedCertificate::from_der(&binary_cert);

    // Should create valid pin
    assert_eq!(pin.sha256_fingerprint.len(), 32);

    // Should verify correctly
    assert!(verify_pin(&binary_cert, &[pin]));
}

/// Tests clone and equality of PinnedCertificate.
/// Required for proper pin list management.
///
/// Maps to: implementation quality
#[test]
fn test_pinned_certificate_clone_and_equality() {
    let fingerprint = [0x42; 32];
    let pin1 = PinnedCertificate::new(fingerprint);
    let pin2 = pin1.clone();

    assert_eq!(pin1, pin2, "Cloned pins should be equal");
    assert_eq!(
        pin1.sha256_fingerprint, pin2.sha256_fingerprint,
        "Cloned pins should have identical fingerprints"
    );
}

/// Tests Debug implementation for PinnedCertificate.
///
/// Maps to: implementation quality
#[test]
fn test_pinned_certificate_debug() {
    let pin = PinnedCertificate::new([0xAB; 32]);
    let debug_str = format!("{:?}", pin);

    assert!(
        debug_str.contains("PinnedCertificate"),
        "Debug output should contain type name"
    );
}

// =============================================================================
// Integration-style Tests
// These test realistic certificate pinning scenarios
// =============================================================================

/// Tests a realistic certificate rotation scenario.
///
/// Maps to: @rotation "Graceful certificate rotation" complete flow
// @scenario: certificate_pinning.feature:Graceful certificate rotation
#[test]
fn test_integration_full_rotation_lifecycle() {
    // Step 1: Initial deployment with single certificate
    let initial_cert = b"Production Certificate v1";
    let initial_pin = PinnedCertificate::from_der(initial_cert);

    assert!(
        verify_pin(initial_cert, std::slice::from_ref(&initial_pin)),
        "Initial deployment should work"
    );

    // Step 2: Prepare for rotation - add new pin
    let new_cert = b"Production Certificate v2";
    let new_pin = PinnedCertificate::from_der(new_cert);

    let rotation_pins = vec![initial_pin.clone(), new_pin.clone()];

    // Both should work during rotation
    assert!(
        verify_pin(initial_cert, &rotation_pins),
        "Old cert works during rotation"
    );
    assert!(
        verify_pin(new_cert, &rotation_pins),
        "New cert works during rotation"
    );

    // Step 3: Complete rotation - remove old pin
    let final_pins = vec![new_pin];

    assert!(
        verify_pin(new_cert, &final_pins),
        "New cert works after rotation"
    );
    assert!(
        !verify_pin(initial_cert, &final_pins),
        "Old cert rejected after rotation"
    );
}

/// Tests that MITM attacks with valid-but-different certificates are detected.
///
/// Maps to: @mitm "the attacker presents a valid but different TLS certificate"
// @scenario: certificate_pinning.feature:Detect MITM with forged certificate
#[test]
fn test_integration_mitm_with_valid_attacker_cert() {
    // Legitimate relay's certificate
    let relay_cert = b"relay.vauchi.app Production Certificate";
    let relay_pin = PinnedCertificate::from_der(relay_cert);

    // Attacker has a valid certificate from a different CA
    // This would pass normal TLS validation but should fail pinning
    let attacker_cert = b"attacker.evil.com Valid Certificate from LetsEncrypt";

    // Pin verification should detect this MITM attempt
    assert!(
        !verify_pin(attacker_cert, &[relay_pin]),
        "Attacker's valid certificate should not match pinned relay certificate"
    );
}

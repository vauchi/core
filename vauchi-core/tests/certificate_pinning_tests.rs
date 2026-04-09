// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate Pinning Tests (SPKI SHA-256)
//!
//! These tests verify the SPKI-based certificate pinning implementation.
//! Each test maps to scenarios from certificate_pinning.feature.
//!
//! Feature reference: features/certificate_pinning.feature

use vauchi_core::network::{PinnedCertificate, verify_pin};

/// Build a minimal DER-encoded X.509 certificate with the given SPKI content.
/// Two certs with the same `spki` produce the same SPKI pin (key continuity).
fn build_test_cert(spki: &[u8]) -> Vec<u8> {
    fn seq(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x30];
        encode_len(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }
    fn tagged(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        encode_len(content.len(), &mut out);
        out.extend_from_slice(content);
        out
    }
    fn encode_len(len: usize, out: &mut Vec<u8>) {
        if len < 0x80 {
            out.push(len as u8);
        } else if len <= 0xFF {
            out.push(0x81);
            out.push(len as u8);
        } else {
            out.push(0x82);
            out.push((len >> 8) as u8);
            out.push((len & 0xFF) as u8);
        }
    }

    let version = tagged(0xA0, &tagged(0x02, &[0x02]));
    let serial = tagged(0x02, &[0x01]);
    let sig_alg = seq(&[0x06, 0x03, 0x55, 0x04, 0x03]);
    let issuer = seq(&[0x31, 0x02, 0x30, 0x00]);
    let validity = seq(&[
        0x17, 0x0D, b'2', b'6', b'0', b'1', b'0', b'1', b'0', b'0', b'0', b'0', b'0', b'0', b'Z',
        0x17, 0x0D, b'2', b'7', b'0', b'1', b'0', b'1', b'0', b'0', b'0', b'0', b'0', b'0', b'Z',
    ]);
    let subject = seq(&[0x31, 0x02, 0x30, 0x00]);
    let spki_seq = seq(spki);

    let tbs = seq(&[
        version, serial, sig_alg, issuer, validity, subject, spki_seq,
    ]
    .concat());
    let cert_sig_alg = seq(&[0x06, 0x03, 0x55, 0x04, 0x03]);
    let cert_sig = tagged(0x03, &[0x00, 0xAB, 0xCD]);

    seq(&[tbs, cert_sig_alg, cert_sig].concat())
}

// =============================================================================
// Pin Format Validation Tests
// =============================================================================

/// @scenario: certificate_pinning :: Pin is SHA-256 hash of DER certificate
#[test]
fn test_pin_format_validation_correct_length() {
    let valid_fingerprint = [0xAB; 32];
    let pin = PinnedCertificate::new(valid_fingerprint);
    assert_eq!(pin.sha256_fingerprint.len(), 32);
}

/// @scenario: certificate_pinning :: SPKI hash is SHA-256
#[test]
fn test_pin_format_spki_uses_sha256() {
    let cert = build_test_cert(b"test-key");
    let pin = PinnedCertificate::from_cert_der(&cert);
    assert!(pin.is_some());
    assert_eq!(pin.unwrap().sha256_fingerprint.len(), 32);
}

/// @scenario: certificate_pinning :: SPKI hash is deterministic
#[test]
fn test_pin_format_deterministic() {
    let cert = build_test_cert(b"determinism-key");
    let pin1 = PinnedCertificate::from_cert_der(&cert).unwrap();
    let pin2 = PinnedCertificate::from_cert_der(&cert).unwrap();
    assert_eq!(pin1, pin2);
}

/// @scenario: certificate_pinning :: Different keys produce different pins
#[test]
fn test_pin_format_collision_resistance() {
    let cert_a = build_test_cert(b"key-alpha");
    let cert_b = build_test_cert(b"key-beta");
    let pin_a = PinnedCertificate::from_cert_der(&cert_a).unwrap();
    let pin_b = PinnedCertificate::from_cert_der(&cert_b).unwrap();
    assert_ne!(pin_a, pin_b);
}

/// @scenario: certificate_pinning :: Garbage input returns None
#[test]
fn test_pin_format_garbage_input() {
    assert!(PinnedCertificate::from_cert_der(b"not a cert").is_none());
    assert!(PinnedCertificate::from_cert_der(b"").is_none());
}

// =============================================================================
// MITM Detection Tests
// =============================================================================

/// @scenario: certificate_pinning :: Detect MITM with forged certificate
#[test]
fn test_mitm_detection_different_certificate() {
    let legit_cert = build_test_cert(b"relay-vauchi-app-key");
    let attacker_cert = build_test_cert(b"attacker-evil-com-key");
    let legit_pin = PinnedCertificate::from_cert_der(&legit_cert).unwrap();

    assert!(!verify_pin(&attacker_cert, &[legit_pin]));
}

/// @scenario: certificate_pinning :: Connection succeeds with matching pin
#[test]
fn test_mitm_detection_matching_certificate_passes() {
    let server_cert = build_test_cert(b"relay-server-key");
    let pin = PinnedCertificate::from_cert_der(&server_cert).unwrap();

    assert!(verify_pin(&server_cert, &[pin]));
}

/// @scenario: certificate_pinning :: Wrong fingerprint fails
#[test]
fn test_mitm_detection_wrong_fingerprint() {
    let server_cert = build_test_cert(b"server-key");
    let fake_pin = PinnedCertificate::new([0xFF; 32]);

    assert!(!verify_pin(&server_cert, &[fake_pin]));
}

/// @scenario: certificate_pinning :: Empty pin list rejects all certificates
#[test]
fn test_mitm_detection_empty_pins_rejects_all() {
    let cert = build_test_cert(b"any-key");
    assert!(!verify_pin(&cert, &[]));
}

/// @scenario: certificate_pinning :: Single byte difference detected
#[test]
fn test_mitm_detection_single_byte_difference() {
    let cert_v1 = build_test_cert(b"server-key-v1.0.0");
    let cert_v2 = build_test_cert(b"server-key-v1.0.1");
    let pin_v1 = PinnedCertificate::from_cert_der(&cert_v1).unwrap();

    assert!(!verify_pin(&cert_v2, &[pin_v1]));
}

// =============================================================================
// Graceful Pin Rotation Tests
// =============================================================================

/// @scenario: certificate_pinning :: Multiple pins allow certificate rotation
#[test]
fn test_graceful_pin_rotation_old_certificate() {
    let old_cert = build_test_cert(b"old-key-2025");
    let new_cert = build_test_cert(b"new-key-2026");
    let old_pin = PinnedCertificate::from_cert_der(&old_cert).unwrap();
    let new_pin = PinnedCertificate::from_cert_der(&new_cert).unwrap();

    let pins = vec![old_pin, new_pin];
    assert!(
        verify_pin(&old_cert, &pins),
        "old cert works during rotation"
    );
}

/// @scenario: certificate_pinning :: New cert works during rotation
#[test]
fn test_graceful_pin_rotation_new_certificate() {
    let old_cert = build_test_cert(b"old-key-2025");
    let new_cert = build_test_cert(b"new-key-2026");
    let old_pin = PinnedCertificate::from_cert_der(&old_cert).unwrap();
    let new_pin = PinnedCertificate::from_cert_der(&new_cert).unwrap();

    let pins = vec![old_pin, new_pin];
    assert!(
        verify_pin(&new_cert, &pins),
        "new cert works during rotation"
    );
}

/// @scenario: certificate_pinning :: Old pin removed after rotation
#[test]
fn test_graceful_pin_rotation_old_pin_removed() {
    let old_cert = build_test_cert(b"old-key-2025");
    let new_cert = build_test_cert(b"new-key-2026");
    let new_pin = PinnedCertificate::from_cert_der(&new_cert).unwrap();

    let pins = vec![new_pin];
    assert!(
        verify_pin(&new_cert, &pins),
        "new cert works after rotation"
    );
    assert!(
        !verify_pin(&old_cert, &pins),
        "old cert rejected after rotation"
    );
}

/// @scenario: certificate_pinning :: Pin matching is order-independent
#[test]
fn test_graceful_pin_rotation_order_independent() {
    let cert_a = build_test_cert(b"key-a");
    let cert_b = build_test_cert(b"key-b");
    let cert_c = build_test_cert(b"key-c");
    let pin_a = PinnedCertificate::from_cert_der(&cert_a).unwrap();
    let pin_b = PinnedCertificate::from_cert_der(&cert_b).unwrap();
    let pin_c = PinnedCertificate::from_cert_der(&cert_c).unwrap();

    assert!(verify_pin(
        &cert_b,
        &[pin_a.clone(), pin_b.clone(), pin_c.clone()]
    ));
    assert!(verify_pin(
        &cert_b,
        &[pin_b.clone(), pin_a.clone(), pin_c.clone()]
    ));
    assert!(verify_pin(
        &cert_b,
        &[pin_c.clone(), pin_a.clone(), pin_b.clone()]
    ));
}

/// @scenario: certificate_pinning :: Many pins scalability
#[test]
fn test_graceful_pin_rotation_many_pins() {
    let target_cert = build_test_cert(b"target-key");
    let target_pin = PinnedCertificate::from_cert_der(&target_cert).unwrap();

    let mut pins: Vec<PinnedCertificate> = (0..100)
        .map(|i| {
            let cert = build_test_cert(format!("key-{i}").as_bytes());
            PinnedCertificate::from_cert_der(&cert).unwrap()
        })
        .collect();
    pins.insert(50, target_pin);

    assert!(verify_pin(&target_cert, &pins));
}

// =============================================================================
// SPKI Pinning Properties
// =============================================================================

/// @scenario: certificate_pinning :: SPKI pin survives cert renewal (same key)
#[test]
fn test_spki_pin_survives_cert_renewal() {
    let shared_key = b"relay-key-unchanged";
    let cert_old = build_test_cert(shared_key);
    let cert_new = build_test_cert(shared_key);

    let pin = PinnedCertificate::from_cert_der(&cert_old).unwrap();
    assert!(
        verify_pin(&cert_new, &[pin]),
        "SPKI pin must survive cert renewal when key is unchanged"
    );
}

/// @scenario: certificate_pinning :: SPKI pin detects key change
#[test]
fn test_spki_pin_detects_key_change() {
    let cert_old = build_test_cert(b"old-relay-key");
    let cert_new = build_test_cert(b"new-relay-key-after-compromise");

    let pin = PinnedCertificate::from_cert_der(&cert_old).unwrap();
    assert!(
        !verify_pin(&cert_new, &[pin]),
        "SPKI pin must detect key change (potential compromise)"
    );
}

// =============================================================================
// Fail-Secure + Edge Cases
// =============================================================================

/// @scenario: certificate_pinning :: Fail-secure when all pins wrong
#[test]
fn test_pinning_disabled_fallback_all_pins_wrong() {
    let cert = build_test_cert(b"server-key");
    let wrong_pins = vec![
        PinnedCertificate::new([0x11; 32]),
        PinnedCertificate::new([0x22; 32]),
        PinnedCertificate::new([0x33; 32]),
    ];
    assert!(!verify_pin(&cert, &wrong_pins));
}

/// @scenario: certificate_pinning :: Clone and equality
#[test]
fn test_pinned_certificate_clone_and_equality() {
    let fingerprint = [0x42; 32];
    let pin1 = PinnedCertificate::new(fingerprint);
    let pin2 = pin1.clone();
    assert_eq!(pin1, pin2);
    assert_eq!(pin1.sha256_fingerprint, pin2.sha256_fingerprint);
}

/// @scenario: certificate_pinning :: Debug representation
#[test]
fn test_pinned_certificate_debug() {
    let pin = PinnedCertificate::new([0xAB; 32]);
    let debug_str = format!("{:?}", pin);
    assert!(debug_str.contains("PinnedCertificate"));
}

// =============================================================================
// Integration: Full Rotation Lifecycle
// =============================================================================

/// @scenario: certificate_pinning :: Graceful certificate rotation (full lifecycle)
#[test]
fn test_integration_full_rotation_lifecycle() {
    let initial_cert = build_test_cert(b"production-key-v1");
    let initial_pin = PinnedCertificate::from_cert_der(&initial_cert).unwrap();

    // Step 1: Initial deployment
    assert!(verify_pin(
        &initial_cert,
        std::slice::from_ref(&initial_pin)
    ));

    // Step 2: Prepare rotation — add new pin
    let new_cert = build_test_cert(b"production-key-v2");
    let new_pin = PinnedCertificate::from_cert_der(&new_cert).unwrap();
    let rotation_pins = vec![initial_pin, new_pin.clone()];

    assert!(
        verify_pin(&initial_cert, &rotation_pins),
        "old cert works during rotation"
    );
    assert!(
        verify_pin(&new_cert, &rotation_pins),
        "new cert works during rotation"
    );

    // Step 3: Complete rotation — remove old pin
    let final_pins = vec![new_pin];
    assert!(
        verify_pin(&new_cert, &final_pins),
        "new cert works after rotation"
    );
    assert!(
        !verify_pin(&initial_cert, &final_pins),
        "old cert rejected after rotation"
    );
}

/// @scenario: certificate_pinning :: MITM with valid attacker cert
#[test]
fn test_integration_mitm_with_valid_attacker_cert() {
    let relay_cert = build_test_cert(b"relay-vauchi-app-production-key");
    let relay_pin = PinnedCertificate::from_cert_der(&relay_cert).unwrap();

    let attacker_cert = build_test_cert(b"attacker-letsencrypt-key");
    assert!(
        !verify_pin(&attacker_cert, &[relay_pin]),
        "Attacker's valid cert must not match pinned relay cert"
    );
}

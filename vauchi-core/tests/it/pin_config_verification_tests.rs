// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "network-http")]

//! Tests for signed pin-config verification (C7 certificate pinning).
//!
//! Uses real Ed25519 keys per ADR-002 (no crypto mocking).

use ed25519_dalek::{Signer, SigningKey};
use vauchi_core::network::http_transport::verify_signed_pin_config;

/// Build a valid signed pin-config payload.
///
/// Wire format: `[64-byte Ed25519 signature][N * 32-byte SPKI fingerprints]`
fn build_signed_payload(signing_key: &SigningKey, pins: &[[u8; 32]]) -> Vec<u8> {
    let pin_data: Vec<u8> = pins.iter().flat_map(|p| p.iter().copied()).collect();
    let sig = signing_key.sign(&pin_data);
    let mut payload = sig.to_bytes().to_vec();
    payload.extend_from_slice(&pin_data);
    payload
}

fn test_keypair() -> (SigningKey, [u8; 32]) {
    let sk = SigningKey::generate(&mut rand::thread_rng());
    let vk_bytes = sk.verifying_key().to_bytes();
    (sk, vk_bytes)
}

// ─── Positive path ──────────────────────────────────────────────────

/// @internal C7: valid signed payload with one pin
#[test]
fn valid_single_pin_passes_verification() {
    let (sk, vk) = test_keypair();
    let pin = [0xAA; 32];
    let payload = build_signed_payload(&sk, &[pin]);

    let result = verify_signed_pin_config(&payload, &vk);
    let pins = result.expect("valid signature must pass verification");

    assert_eq!(pins.len(), 1, "must return exactly one pin");
    assert_eq!(
        pins[0].sha256_fingerprint, pin,
        "returned pin must match input"
    );
}

/// @internal C7: valid signed payload with multiple pins
#[test]
fn valid_multiple_pins_passes_verification() {
    let (sk, vk) = test_keypair();
    let pin_a = [0xAA; 32];
    let pin_b = [0xBB; 32];
    let pin_c = [0xCC; 32];
    let payload = build_signed_payload(&sk, &[pin_a, pin_b, pin_c]);

    let result = verify_signed_pin_config(&payload, &vk);
    let pins = result.expect("valid signature must pass verification");

    assert_eq!(pins.len(), 3, "must return all three pins");
    assert_eq!(pins[0].sha256_fingerprint, pin_a);
    assert_eq!(pins[1].sha256_fingerprint, pin_b);
    assert_eq!(pins[2].sha256_fingerprint, pin_c);
}

// ─── Negative paths ─────────────────────────────────────────────────

/// @internal C7: tampered pin data (flipped byte) must be rejected
#[test]
fn tampered_pin_data_rejected() {
    let (sk, vk) = test_keypair();
    let pin = [0xAA; 32];
    let mut payload = build_signed_payload(&sk, &[pin]);

    // Flip a byte in the pin data (byte 64 = first byte of pin data)
    payload[64] ^= 0xFF;

    let result = verify_signed_pin_config(&payload, &vk);
    let err = result.expect_err("tampered payload must be rejected");
    assert!(
        err.to_string().contains("signature verification failed"),
        "error must mention signature verification, got: {err}"
    );
}

/// @internal C7: wrong verify key must be rejected
#[test]
fn wrong_verify_key_rejected() {
    let (sk, _vk) = test_keypair();
    let (_, wrong_vk) = test_keypair();
    let pin = [0xAA; 32];
    let payload = build_signed_payload(&sk, &[pin]);

    let result = verify_signed_pin_config(&payload, &wrong_vk);
    let err = result.expect_err("wrong key must be rejected");
    assert!(
        err.to_string().contains("signature verification failed"),
        "error must mention signature verification, got: {err}"
    );
}

/// @internal C7: body shorter than 96 bytes must be rejected
#[test]
fn too_short_body_rejected() {
    let (_, vk) = test_keypair();

    // 95 bytes: one byte short of minimum (64 sig + 32 pin)
    let short_body = vec![0u8; 95];
    let result = verify_signed_pin_config(&short_body, &vk);
    let err = result.expect_err("too-short body must be rejected");
    assert!(
        err.to_string().contains("too short"),
        "error must mention 'too short', got: {err}"
    );
}

/// @internal C7: empty body must be rejected
#[test]
fn empty_body_rejected() {
    let (_, vk) = test_keypair();

    let result = verify_signed_pin_config(&[], &vk);
    let err = result.expect_err("empty body must be rejected");
    assert!(
        err.to_string().contains("too short"),
        "error must mention 'too short', got: {err}"
    );
}

/// @internal C7: misaligned pin data (not multiple of 32) must be rejected
#[test]
fn misaligned_pin_data_rejected() {
    let (sk, vk) = test_keypair();
    let pin = [0xAA; 32];
    let mut payload = build_signed_payload(&sk, &[pin]);

    // Append 1 extra byte — pin data is now 33 bytes (not a multiple of 32)
    payload.push(0x00);

    let result = verify_signed_pin_config(&payload, &vk);
    let err = result.expect_err("misaligned pin data must be rejected");
    assert!(
        err.to_string().contains("multiple of 32"),
        "error must mention alignment, got: {err}"
    );
}

/// @internal C7: signature-only body (64 bytes, no pins) must be rejected
#[test]
fn signature_only_no_pins_rejected() {
    let (_, vk) = test_keypair();

    // Exactly 64 bytes = valid signature length but zero pins
    let sig_only = vec![0u8; 64];
    let result = verify_signed_pin_config(&sig_only, &vk);
    let err = result.expect_err("signature-only body (no pins) must be rejected");
    assert!(
        err.to_string().contains("too short"),
        "error must mention 'too short', got: {err}"
    );
}

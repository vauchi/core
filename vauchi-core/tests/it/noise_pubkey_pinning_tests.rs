// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Noise NK pubkey pinning during relay connection (Phase 1D).
//!
//! When a contact's relay Noise pubkey is known (pinned during exchange),
//! the client must verify the relay's actual pubkey matches.

use vauchi_core::network::relay_url::verify_relay_noise_pubkey;

// ── Matching pubkey ────────────────────────────────────────────────

// @scenario: noise_protocol :: Noise NK handshake with relay
#[test]
fn matching_pubkey_accepted() {
    let expected = [42u8; 32];
    let actual = [42u8; 32];
    verify_relay_noise_pubkey(Some(&expected), &actual).expect("expected success");
}

#[test]
fn no_pinned_pubkey_accepted() {
    // When no pubkey is pinned (TOFU), any relay pubkey is accepted
    let actual = [99u8; 32];
    verify_relay_noise_pubkey(None, &actual).expect("expected success");
}

// ── Mismatched pubkey ──────────────────────────────────────────────

// @scenario: noise_protocol :: Handshake fails with wrong relay key
#[test]
fn mismatched_pubkey_rejected() {
    let expected = [42u8; 32];
    let actual = [99u8; 32];
    let err = verify_relay_noise_pubkey(Some(&expected), &actual).unwrap_err();
    assert!(
        matches!(
            err,
            vauchi_core::network::relay_url::RelayUrlError::NoisePubkeyMismatch
        ),
        "Expected NoisePubkeyMismatch, got: {err:?}"
    );
}

#[test]
fn single_byte_difference_rejected() {
    let expected = [42u8; 32];
    let mut actual = [42u8; 32];
    actual[31] = 43; // Differ in last byte only
    let err = verify_relay_noise_pubkey(Some(&expected), &actual).unwrap_err();
    assert!(matches!(
        err,
        vauchi_core::network::relay_url::RelayUrlError::NoisePubkeyMismatch
    ));
}

#[test]
fn all_zeros_vs_all_ones_rejected() {
    let expected = [0u8; 32];
    let actual = [0xFF; 32];
    verify_relay_noise_pubkey(Some(&expected), &actual).expect_err("expected error");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::signing
//! Extracted from signing.rs

use vauchi_core::crypto::*;

// @scenario: identity_management.feature:Create new identity on first launch
// @scenario: security.feature:Correct algorithms used
#[test]
fn test_keypair_generation() {
    let kp = SigningKeyPair::generate();
    assert_eq!(kp.public_key().as_bytes().len(), 32);
}

// @scenario: security.feature:Contact card signatures verified
#[test]
fn test_sign_verify() {
    let kp = SigningKeyPair::generate();
    let msg = b"test message";
    let sig = kp.sign(msg);
    assert!(kp.public_key().verify(msg, &sig));
}

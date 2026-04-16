// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for identity::password
//! Extracted from password.rs

use vauchi_core::identity::password::{PasswordStrength, validate_password};
use zxcvbn::Score;

// @internal
#[test]
fn test_password_strength_from_score() {
    assert_eq!(
        PasswordStrength::from(Score::Zero),
        PasswordStrength::TooWeak
    );
    assert_eq!(PasswordStrength::from(Score::One), PasswordStrength::Weak);
    assert_eq!(PasswordStrength::from(Score::Two), PasswordStrength::Fair);
    assert_eq!(
        PasswordStrength::from(Score::Three),
        PasswordStrength::Strong
    );
    assert_eq!(
        PasswordStrength::from(Score::Four),
        PasswordStrength::VeryStrong
    );
}

// @scenario: identity_management :: Backup password requirements
// @internal
#[test]
fn test_short_password() {
    validate_password("short").expect_err("expected error");
    validate_password("").expect_err("expected error");
    validate_password("1234567").expect_err("expected error");
}

// @scenario: identity_management :: Backup password requirements
// @internal
#[test]
fn test_common_passwords() {
    validate_password("password").expect_err("expected error");
    validate_password("12345678").expect_err("expected error");
    validate_password("qwertyui").expect_err("expected error");
}

// @scenario: identity_management :: Backup password requirements
// @internal
#[test]
fn test_strong_passphrase() {
    let result = validate_password("correct-horse-battery-staple");
    result.expect("expected success");
}

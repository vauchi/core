// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for identity::password

use vauchi_core::identity::password::{PasswordStrength, validate_password};

// @internal
#[test]
fn test_password_strength_levels_are_distinct() {
    // Sanity check that the enum discriminates the four levels used by the API.
    assert_ne!(PasswordStrength::TooWeak, PasswordStrength::Weak);
    assert_ne!(PasswordStrength::Weak, PasswordStrength::Fair);
    assert_ne!(PasswordStrength::Fair, PasswordStrength::Strong);
    assert_ne!(PasswordStrength::Strong, PasswordStrength::VeryStrong);
}

// @internal
#[test]
fn test_password_strength_mapping() {
    // Too short -> rejected before scoring.
    validate_password("short").expect_err("expected error");

    // Common/sequential patterns at minimum length are rejected.
    validate_password("password").expect_err("expected error");
    validate_password("12345678").expect_err("expected error");
    validate_password("qwertyui").expect_err("expected error");
    validate_password("aaaaaaaa").expect_err("expected error");

    // Strong passphrase passes and maps to Strong/VeryStrong.
    let strength = validate_password("correct-horse-battery-staple").unwrap();
    assert!(matches!(
        strength,
        PasswordStrength::Strong | PasswordStrength::VeryStrong
    ));

    // Long random password with all character classes is VeryStrong.
    let strength = validate_password("Zq!9xK#mP$2vL&nW@4rT^8jYf").unwrap();
    assert!(matches!(strength, PasswordStrength::VeryStrong));
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

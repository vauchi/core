// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Additional identity::password tests for full coverage of PasswordStrength
//! and password_feedback.

use vauchi_core::identity::password::{PasswordStrength, password_feedback, validate_password};

// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_strong_password() {
    let result = validate_password("correct-horse-battery-staple");
    assert!(result.is_ok(), "expected success");
    let strength = result.unwrap();
    assert!(matches!(
        strength,
        PasswordStrength::Strong | PasswordStrength::VeryStrong
    ));
}

// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_very_strong_password() {
    let result = validate_password("Zq!9xK#mP$2vL&nW@4rT^8jYf");
    assert!(result.is_ok(), "expected success");
    let strength = result.unwrap();
    assert!(matches!(strength, PasswordStrength::VeryStrong));
}

// @scenario: identity_management :: Backup password requirements
// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_weak_password() {
    let result = validate_password("password");
    assert!(result.is_err(), "expected error");
}

// @scenario: identity_management :: Backup password requirements
// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_too_short() {
    let result = validate_password("Ab1!x");
    assert!(result.is_err(), "expected error");
}

// @scenario: identity_management :: Backup password requirements
// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_common_password() {
    let result = validate_password("12345678");
    assert!(result.is_err(), "expected error");
}

// @scenario: identity_management :: Backup password requirements
// @scenario: identity_management :: Password strength validation
// @internal
#[test]
fn test_validate_exactly_min_length_but_weak() {
    let result = validate_password("aaaaaaaa");
    assert!(result.is_err(), "expected error");
}

// @internal
#[test]
fn test_password_strength_from_internal_score() {
    // The internal estimator maps scores 0-4 to the same enum variants.
    assert!(matches!(
        validate_password("12345678").unwrap_err(),
        vauchi_core::identity::IdentityError::WeakPassword
    ));
    assert!(matches!(
        validate_password("correct-horse-battery-staple").unwrap(),
        PasswordStrength::Strong | PasswordStrength::VeryStrong
    ));
}

// @internal
// Regression for audit-08 CRIT-A: the prior assert
// `!feedback.is_empty() || feedback.is_empty()` was a textbook
// tautology that passed for any output. Weak passwords must always
// return at least one suggestion; assert non-empty feedback as the
// actual contract.
#[test]
fn test_password_feedback_weak_password() {
    let feedback = password_feedback("password123");
    assert!(
        !feedback.is_empty(),
        "password_feedback must return at least one suggestion for the canonical \
         weak password \"password123\"; got empty feedback"
    );
}

// @internal
#[test]
fn test_password_feedback_strong_password() {
    // allow(zero_assertions): No-panic coverage test — strong passwords may have empty feedback
    let feedback = password_feedback("correct-horse-battery-staple");
    let _ = feedback;
}

// @internal
#[test]
fn test_password_feedback_very_weak() {
    // allow(zero_assertions): No-panic coverage test
    let feedback = password_feedback("aaa");
    let _ = feedback;
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A duress PIN must be exactly six digits on the way in.
//!
//! Every surface calls this a PIN — the CLI subcommand, the mobile
//! dispatch, and a fixed six-box `PinInput` — but nothing enforced it.
//! `setup_duress` checked only that the value differed from the normal
//! password, so any string became a duress credential. A device was found
//! carrying the literal string `undefined`, typed there by a test flow
//! whose variable was unset
//! (`backlog/2026-08-09-duress-pin-accepts-any-string-on-paste.md`).
//!
//! The constraint belongs on the write path only. Validating on
//! `verify`/`authenticate` would lock out anyone whose existing duress PIN
//! predates the rule — under coercion, at the moment it is needed. The
//! last test pins that asymmetry, and it is the one that must never be
//! "simplified" away.

use crate::common;
use common::helpers::create_vauchi_with_identity;
use vauchi_core::{AppPasswordConfig, AuthMode, AuthResult};

/// Every rejected shape, so a future relaxation has to delete a named case
/// rather than quietly widen a regex (CC-14).
fn rejected_pins() -> Vec<(&'static str, &'static str)> {
    vec![
        ("", "empty"),
        ("12345", "five digits — one short"),
        ("1234567", "seven digits — one long"),
        ("abcdef", "six letters"),
        ("undefined", "the value observed on a real device"),
        ("12345a", "five digits and a letter"),
        ("12 345", "embedded space"),
        (" 12345", "leading space"),
        ("123456 ", "trailing space"),
        ("१२३४५६", "Devanagari digits — not ASCII"),
        ("１２３４５６", "fullwidth digits — not ASCII"),
        ("12345\n", "trailing newline"),
        ("-12345", "sign character"),
    ]
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn setup_duress_password_rejects_anything_but_six_ascii_digits() {
    for (pin, why) in rejected_pins() {
        let mut wb = create_vauchi_with_identity("Alice");
        wb.setup_app_password("my-pin-1234")
            .expect("app password should be set first");

        let result = wb.setup_duress_password(pin);

        assert!(
            result.is_err(),
            "duress PIN {pin:?} ({why}) must be rejected — a duress \
             credential the user cannot reproduce fails exactly when it \
             is needed"
        );
    }
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn setup_duress_password_accepts_six_ascii_digits() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234")
        .expect("app password should be set first");

    wb.setup_duress_password("654321")
        .expect("six ASCII digits is the documented shape and must be accepted");

    assert!(
        wb.is_duress_enabled().expect("duress status should load"),
        "a valid PIN must actually enable duress, not merely be accepted"
    );
    assert_eq!(
        wb.authenticate("654321")
            .expect("the accepted PIN must authenticate"),
        AuthMode::Duress,
        "the stored PIN must resolve to Duress mode"
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn rejected_duress_pin_leaves_duress_disabled() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234")
        .expect("app password should be set first");

    let _ = wb.setup_duress_password("undefined");

    assert!(
        !wb.is_duress_enabled().expect("duress status should load"),
        "a rejected PIN must not half-enable duress — reporting Enabled \
         while the credential is garbage is the failure being fixed"
    );
}

// @scenario: duress_mode :: Duress credential shows decoy contacts
#[test]
fn verification_still_accepts_a_pin_stored_before_the_rule_existed() {
    // Built through the low-level primitive deliberately: it models a
    // credential already on disk, which the new rule must not invalidate.
    // The value must NOT satisfy the new rule — that is the whole point.
    // A sweep once replaced it with a conforming PIN, leaving a test that
    // passed while proving nothing.
    const LEGACY_PIN: &str = "duress-999";

    let mut config = AppPasswordConfig::create("my-pin-1234").expect("config should build");
    config
        .setup_duress(LEGACY_PIN)
        .expect("the storage primitive stays unconstrained by design");

    assert!(
        vauchi_core::validate_duress_pin(LEGACY_PIN).is_err(),
        "this test is meaningless unless the stored PIN violates the new rule"
    );
    assert_eq!(
        config.verify(LEGACY_PIN),
        AuthResult::Duress,
        "an existing duress PIN must keep working — validating the read \
         path would lock someone out under coercion"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret a shell seals behind the OS's own user-presence check
//! (biometric or device credential): Core keeps only its digest, enrolls
//! it once per device, and confirms a revealed secret against it (#712 D2).

use vauchi_core::api::Vauchi;

const SECRET: [u8; 32] = [7; 32];

// @internal
#[test]
fn an_enrolled_secret_confirms_and_no_other_does() {
    let vauchi = Vauchi::in_memory().unwrap();
    assert!(!vauchi.has_user_presence_secret().unwrap());

    assert!(vauchi.enroll_user_presence_secret(&SECRET).unwrap());

    assert!(vauchi.has_user_presence_secret().unwrap());
    assert!(vauchi.confirm_user_presence_secret(&SECRET).unwrap());
    let mut other = SECRET;
    other[31] ^= 1;
    assert!(!vauchi.confirm_user_presence_secret(&other).unwrap());
}

// A second enrolment would let whoever can send events replace the secret
// the keystore guards; only the first one counts.
// @internal
#[test]
fn a_second_enrolment_is_refused_and_keeps_the_first() {
    let vauchi = Vauchi::in_memory().unwrap();
    assert!(vauchi.enroll_user_presence_secret(&SECRET).unwrap());

    assert!(!vauchi.enroll_user_presence_secret(&[9; 32]).unwrap());

    assert!(vauchi.confirm_user_presence_secret(&SECRET).unwrap());
    assert!(!vauchi.confirm_user_presence_secret(&[9; 32]).unwrap());
}

// @internal
#[test]
fn nothing_confirms_before_enrolment() {
    let vauchi = Vauchi::in_memory().unwrap();

    assert!(!vauchi.confirm_user_presence_secret(&SECRET).unwrap());
}

// The secret arrives from a shell event: only 16 to 64 bytes are a secret.
// @internal
#[test]
fn a_secret_of_the_wrong_size_is_refused() {
    let vauchi = Vauchi::in_memory().unwrap();
    for secret in [vec![], vec![1; 15], vec![1; 65]] {
        assert!(
            !vauchi.enroll_user_presence_secret(&secret).unwrap(),
            "{} bytes must not enroll",
            secret.len()
        );
    }
    assert!(!vauchi.has_user_presence_secret().unwrap());
    assert!(vauchi.enroll_user_presence_secret(&SECRET).unwrap());
    assert!(!vauchi.confirm_user_presence_secret(&[]).unwrap());
}

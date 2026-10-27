// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The BLE role-tiebreak attempt token (vauchi/private#726).

use vauchi_core::exchange::{attempt_token_from_oob, fresh_attempt_token};

// @internal
#[test]
fn every_attempt_gets_its_own_token() {
    let first = fresh_attempt_token();
    let second = fresh_attempt_token();
    assert_ne!(first, second);
    assert_ne!(first[..2], second[..2]);
}

// @internal
#[test]
fn the_glance_token_is_a_function_of_the_qr_token_and_not_the_qr_token_itself() {
    let qr_token = [7u8; 32];
    let displayer = attempt_token_from_oob(&qr_token);
    let scanner = attempt_token_from_oob(&qr_token);
    assert_eq!(displayer, scanner);
    assert_ne!(displayer, qr_token);
    assert_ne!(displayer, attempt_token_from_oob(&[8u8; 32]));
}

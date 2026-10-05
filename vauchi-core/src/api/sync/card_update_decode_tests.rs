// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Only a CEK-versioned payload is unwrapped; anything else, empty
//! included, is refused by version (vauchi/private#522).

use super::{CardUpdateError, decode_versioned_payload};

fn refusal(plaintext: &[u8]) -> String {
    match decode_versioned_payload(plaintext) {
        Err(CardUpdateError::InvalidPayload(reason)) => reason,
        other => panic!("expected InvalidPayload, got {:?}", other.map(|(p, _)| p)),
    }
}

// @internal
#[test]
fn an_empty_or_unversioned_payload_is_refused_by_version() {
    assert_eq!(refusal(&[]), "unknown payload version");
    assert_eq!(refusal(&[0x01, 0xAA, 0xBB]), "unknown payload version");
}

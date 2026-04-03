// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay-mediated exchange — SAS verification.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Derive a 6-digit SAS verification code from the
/// shared X3DH secret and both parties' identity keys.
///
/// Uses HMAC-SHA256 with domain separation. Identity
/// keys are sorted before hashing so both parties
/// compute the same value regardless of who initiated.
pub fn derive_sas(
    shared_secret: &[u8; 32],
    identity_a: &[u8; 32],
    identity_b: &[u8; 32],
) -> String {
    let mut mac = HmacSha256::new_from_slice(shared_secret).expect("HMAC accepts any key length");
    mac.update(b"vauchi-relay-exchange-sas-v1");
    if identity_a <= identity_b {
        mac.update(identity_a);
        mac.update(identity_b);
    } else {
        mac.update(identity_b);
        mac.update(identity_a);
    }
    let tag = mac.finalize().into_bytes();
    let v = u32::from_be_bytes([tag[0], tag[1], tag[2], tag[3]]) % 1_000_000;
    format!("{:03}-{:03}", v / 1000, v % 1000)
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Relay-mediated exchange — SAS verification.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

// @claim: S9
/// Derive a full authentication fingerprint from the shared X3DH secret
/// and both identity keys. The public function name is retained for callers.
///
/// The relay can select both transcripts adaptively, so truncation to a
/// short comparison code permits a birthday collision search (private#925).
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
    mac.finalize()
        .into_bytes()
        .chunks_exact(4)
        .map(hex::encode)
        .collect::<Vec<_>>()
        .join("-")
}

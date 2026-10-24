// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Contact reciprocity proof derivation.

use crate::crypto::kdf::HKDF;
use zeroize::Zeroizing;

/// HKDF domain-separation label for reciprocity confirmation tokens (ADR-007).
const DOMAIN_RECIPROCITY_CONFIRM: &[u8] = b"vauchi-reciprocity-confirm-v1";

/// A `(our_token, expected_their_token)` reciprocity confirmation pair.
pub type ConfirmationTokenPair = (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>);

/// Derive the `(our_token, expected_their_token)` confirmation pair from the
/// exchange shared secret and both parties' identity signing keys.
///
/// Each token binds exactly one identity key via the HKDF `info`, so the pair
/// cross-matches between peers:
/// `derive(secret, a, b).0 == derive(secret, b, a).1`. The tokens are secret-
/// bound (a different shared secret yields different tokens) and self-
/// asymmetric (`our_token != expected_their_token`, echo protection).
pub fn derive_confirmation_tokens(
    shared_secret: &[u8],
    our_id: &[u8],
    their_id: &[u8],
) -> ConfirmationTokenPair {
    let our_info = [DOMAIN_RECIPROCITY_CONFIRM, our_id].concat();
    let their_info = [DOMAIN_RECIPROCITY_CONFIRM, their_id].concat();
    let our_token = HKDF::derive_key(None, shared_secret, &our_info);
    let their_token = HKDF::derive_key(None, shared_secret, &their_info);
    (our_token, their_token)
}

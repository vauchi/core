// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reciprocity confirmation token derivation (design spec §2).
//!
//! A confirmation token proves a party completed the exchange from its own
//! side. It is derived from the exchange's shared secret, domain-separated,
//! and bound to one party's identity signing key — so the pair *cross-matches*
//! across the two peers: A's `our_token` (which binds A's id) equals B's
//! `expected_their_token` (which also binds A's id, from the same shared
//! secret). Transport-agnostic by construction: QR, BLE, and multi-stage all
//! derive the same way from their own shared secret, so one shared
//! `ReciprocityConfirmer` can run over any channel (relay escrow for QR/Cable,
//! the native bidirectional link for BLE/multi-stage).

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::crypto::SymmetricKey;
use crate::crypto::encryption;
use crate::crypto::kdf::HKDF;

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

/// AEAD associated-data domain tag for the reciprocity confirmation ack
/// (design P1 + its security review). Binds the ack so it cannot be confused
/// with a card / transport payload; the appended sender||receiver identities
/// bind direction. A mismatch fails the AEAD tag.
const ACK_AAD_DOMAIN: &[u8] = b"vauchi-reciprocity-ack-v1";

/// Wire version byte for the reciprocity ack.
const ACK_VERSION: u8 = 0x01;

/// Build an encrypted reciprocity confirmation ack (design P1), transport-
/// agnostic: `our_token` AEAD-sealed under `aead_key` (the exchange's agreed
/// key — the BLE session key or the multi-stage transport key) with a
/// direction-bound AAD (`domain || our_id || their_id`). Wire form is
/// `version || ciphertext`. The peer verifies it with [`verify_ack`]. MUST be
/// emitted only after durable persist (G1); the caller enforces that ordering.
pub fn build_ack(
    aead_key: &SymmetricKey,
    our_token: &[u8; 32],
    our_id: &[u8],
    their_id: &[u8],
) -> Option<Vec<u8>> {
    let aad = [ACK_AAD_DOMAIN, our_id, their_id].concat();
    let ciphertext = encryption::encrypt_with_ad(aead_key, our_token, &aad).ok()?;
    let mut ack = Vec::with_capacity(1 + ciphertext.len());
    ack.push(ACK_VERSION);
    ack.extend_from_slice(&ciphertext);
    Some(ack)
}

/// Verify a reciprocity ack against `expected_their_token`. Decrypts under
/// `aead_key` + the mirrored AAD (`domain || their_id || our_id`), then compares
/// the plaintext to the expected token in constant time. `Ok(true)` = Confirmed;
/// `Ok(false)` = token mismatch (caller keeps Pending — fail-safe); `Err(())` =
/// malformed or undecryptable. A tampered ack, a MITM's token (different agreed
/// key), or a replayed foreign-session ack all fail here, so a false Confirmed
/// is impossible.
// The error is intentionally info-free: structural failures (bad version,
// undecryptable) are all treated identically by callers (they map to a
// transport error) — only `Ok(true)`/`Ok(false)` carries a decision.
#[allow(clippy::result_unit_err)]
pub fn verify_ack(
    aead_key: &SymmetricKey,
    expected_their_token: &[u8; 32],
    our_id: &[u8],
    their_id: &[u8],
    ack: &[u8],
) -> Result<bool, ()> {
    let (&version, ciphertext) = ack.split_first().ok_or(())?;
    if version != ACK_VERSION {
        return Err(());
    }
    let aad = [ACK_AAD_DOMAIN, their_id, our_id].concat();
    let plaintext = encryption::decrypt_with_ad(aead_key, ciphertext, &aad).map_err(|_| ())?;
    Ok(bool::from(
        plaintext.as_slice().ct_eq(expected_their_token.as_slice()),
    ))
}

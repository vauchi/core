// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Origin-device hint: an opaque, recipient-encrypted marker that lets a
//! receiver select the sender's device-pair ratchet before decrypting, without
//! the transport learning the sender edge.
//!
//! The production HTTP transport discards the sender-device identity, so a
//! secondary device's card delta cannot be routed to the right
//! `(contact, sender_device)` ratchet on the receiver. The hint restores that
//! selection privately: it is `AEAD` over the sender device id, keyed by a
//! domain-separated derivation of the pair `shared_key`, bound to the recipient
//! mailbox token and the exact ciphertext.
//!
//! Design: `_private/docs/designs/2026-07-27-origin-device-hint-design.md`.
//!
//! Security note — the hint is a *selection* hint only. It is never the
//! authority for the origin device: successful ratchet decryption is. A forged
//! or swapped hint at worst selects a session that will not decrypt, and the
//! receiver falls through to the legacy `[0;32]` path. Binding the ciphertext
//! into the AAD is what makes a relay-swapped hint fail to open (rather than
//! selecting the wrong session and dropping a legitimate blob).

use base64::Engine;
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use sha2::{Digest, Sha256};

use crate::crypto::kdf::HKDF;
use crate::rng::SecureRng;

/// Domain-separation label for both the HKDF key derivation and the AAD prefix.
const HINT_DOMAIN: &[u8] = b"vauchi/origin-hint/v1";
/// Hint plaintext format version (first plaintext byte).
const HINT_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const DEVICE_ID_LEN: usize = 32;
/// Plaintext layout: `version(1) || scope(1) || sender_device_id(32)`.
const PLAINTEXT_LEN: usize = 2 + DEVICE_ID_LEN;
const POLY1305_TAG_LEN: usize = 16;

/// Default scope byte for a device-pair card delta. Reserved for future format
/// evolution (e.g. distinguishing bootstrap traffic); the receiver does not
/// branch on it today.
pub const SCOPE_CARD_DELTA: u8 = 0;

fn hint_key(shared_key: &[u8; 32]) -> zeroize::Zeroizing<[u8; 32]> {
    HKDF::derive_key(None, shared_key, HINT_DOMAIN)
}

/// AAD = `domain || mailbox_token || SHA-256(ciphertext)`. Binding the
/// ciphertext is what defeats a relay lifting one blob's hint onto another.
fn hint_aad(mailbox_token: &str, ciphertext: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(ciphertext);
    let mut aad = Vec::with_capacity(HINT_DOMAIN.len() + mailbox_token.len() + digest.len());
    aad.extend_from_slice(HINT_DOMAIN);
    aad.extend_from_slice(mailbox_token.as_bytes());
    aad.extend_from_slice(&digest);
    aad
}

/// Seals `sender_device_id` into a base64 hint bound to `mailbox_token` and
/// `ciphertext`. Returns `None` only on the (infeasible) AEAD failure.
pub fn seal_origin_hint(
    shared_key: &[u8; 32],
    sender_device_id: &[u8; 32],
    mailbox_token: &str,
    ciphertext: &[u8],
    scope: u8,
    rng: &dyn SecureRng,
) -> Option<String> {
    let key = hint_key(shared_key);
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);

    let mut plaintext = Vec::with_capacity(PLAINTEXT_LEN);
    plaintext.push(HINT_VERSION);
    plaintext.push(scope);
    plaintext.extend_from_slice(sender_device_id);

    let aad = hint_aad(mailbox_token, ciphertext);
    let sealed = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .ok()?;

    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Some(base64::engine::general_purpose::STANDARD.encode(out))
}

/// Opens a hint, returning the sender device id when it authenticates against
/// this `shared_key`, `mailbox_token`, and `ciphertext`. Any mismatch (wrong
/// relationship, swapped blob, tampered bytes, unknown version) returns `None`
/// so the caller falls back to the legacy `[0;32]` path.
pub fn open_origin_hint(
    shared_key: &[u8; 32],
    mailbox_token: &str,
    ciphertext: &[u8],
    hint_b64: &str,
) -> Option<[u8; 32]> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(hint_b64)
        .ok()?;
    if raw.len() != NONCE_LEN + PLAINTEXT_LEN + POLY1305_TAG_LEN {
        return None;
    }
    let (nonce_bytes, sealed) = raw.split_at(NONCE_LEN);

    let key = hint_key(shared_key);
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    let aad = hint_aad(mailbox_token, ciphertext);
    let plaintext = cipher
        .decrypt(
            chacha20poly1305::XNonce::from_slice(nonce_bytes),
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .ok()?;

    if plaintext.len() != PLAINTEXT_LEN || plaintext[0] != HINT_VERSION {
        return None;
    }
    let mut device_id = [0u8; DEVICE_ID_LEN];
    device_id.copy_from_slice(&plaintext[2..]);
    Some(device_id)
}

// INLINE_TEST_REQUIRED: adversarial crypto unit tests (tamper, wrong-key,
// swap-defense) exercised beside the seal/open primitive they protect.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::DeterministicRng;

    const SHARED: [u8; 32] = [7u8; 32];
    const DEVICE: [u8; 32] = [42u8; 32];
    const TOKEN: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
    const CT: &[u8] = b"an-opaque-ratchet-message";

    fn rng() -> DeterministicRng {
        DeterministicRng::from_seed(0xF4)
    }

    // @internal
    #[test]
    fn seal_then_open_recovers_the_device_id() {
        let hint = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, SCOPE_CARD_DELTA, &rng()).unwrap();
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, &hint), Some(DEVICE));
    }

    // @internal
    #[test]
    fn two_seals_of_the_same_device_are_unlinkable() {
        // One long-lived RNG (as in production) → the nonce advances between
        // messages, so two hints for the same device differ.
        let rng = rng();
        let a = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng).unwrap();
        let b = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng).unwrap();
        assert_ne!(a, b, "fresh nonce must make repeated hints differ");
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, &a), Some(DEVICE));
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, &b), Some(DEVICE));
    }

    // @internal
    #[test]
    fn hint_swapped_onto_a_different_ciphertext_fails_to_open() {
        // The swap-defense regression test: a relay lifting this hint onto
        // another blob (different ciphertext) must fail the AAD check.
        let hint = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng()).unwrap();
        assert_eq!(
            open_origin_hint(&SHARED, TOKEN, b"a-different-blob", &hint),
            None
        );
    }

    // @internal
    #[test]
    fn hint_replayed_to_a_different_mailbox_fails_to_open() {
        let hint = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng()).unwrap();
        let other_token = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        assert_eq!(open_origin_hint(&SHARED, other_token, CT, &hint), None);
    }

    // @internal
    #[test]
    fn wrong_relationship_key_fails_to_open() {
        let hint = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng()).unwrap();
        assert_eq!(open_origin_hint(&[9u8; 32], TOKEN, CT, &hint), None);
    }

    // @internal
    #[test]
    fn tampered_hint_bytes_fail_to_open() {
        let hint = seal_origin_hint(&SHARED, &DEVICE, TOKEN, CT, 0, &rng()).unwrap();
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(&hint)
            .unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        let tampered = base64::engine::general_purpose::STANDARD.encode(raw);
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, &tampered), None);
    }

    // @internal
    #[test]
    fn wrong_length_or_garbage_hint_is_rejected() {
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, "not-base64!!!"), None);
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, ""), None);
        let short = base64::engine::general_purpose::STANDARD.encode([0u8; 10]);
        assert_eq!(open_origin_hint(&SHARED, TOKEN, CT, &short), None);
    }
}

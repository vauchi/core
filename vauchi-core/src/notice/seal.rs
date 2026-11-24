// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The seal of a 96-byte notice (ADR-019 amendment 2026-11-24).
//!
//! XChaCha20-Poly1305 under a key derived from the notice's message key,
//! with a fixed nonce: a notice has no room for one, and each message key
//! seals exactly one plaintext. That single use is what the seal rests
//! on; a key sealing two different plaintexts would expose their XOR and
//! let the Poly1305 key forge (vauchi/private#1388).

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use zeroize::Zeroizing;

use super::keys::Keyed;
use crate::crypto::HKDF;

pub const TAG_BYTES: usize = 16;
pub const PLAINTEXT_BYTES: usize = 64;
pub const AUTHENTICATOR_BYTES: usize = 16;
pub const NOTICE_BYTES: usize = TAG_BYTES + PLAINTEXT_BYTES + AUTHENTICATOR_BYTES;

const FIXED_NONCE: [u8; 24] = [0u8; 24];

fn cipher(message_key: &[u8; 32]) -> XChaCha20Poly1305 {
    let key = HKDF::derive_array::<32>(None, message_key, b"Vauchi_Notice_Seal_v8|aead");
    XChaCha20Poly1305::new(key.as_ref().into())
}

/// Seals a plaintext where `at` says: `tag ‖ ciphertext ‖ authenticator`,
/// with the tag as associated data.
pub fn seal(at: &Keyed, plaintext: &[u8; PLAINTEXT_BYTES]) -> [u8; NOTICE_BYTES] {
    let mut notice = [0u8; NOTICE_BYTES];
    let (tag_part, sealed) = notice.split_at_mut(TAG_BYTES);
    let (ciphertext, authenticator) = sealed.split_at_mut(PLAINTEXT_BYTES);
    tag_part.copy_from_slice(&at.tag);
    ciphertext.copy_from_slice(plaintext);
    let computed = cipher(&at.key)
        .encrypt_in_place_detached((&FIXED_NONCE).into(), &at.tag, ciphertext)
        .unwrap_or_else(|_| unreachable!("a 64-byte plaintext is within XChaCha20's limit"));
    authenticator.copy_from_slice(&computed);
    notice
}

/// The plaintext, or `None` when the notice is not what `at`'s key
/// sealed at `at`'s tag.
pub fn open(at: &Keyed, notice: &[u8; NOTICE_BYTES]) -> Option<Zeroizing<[u8; PLAINTEXT_BYTES]>> {
    let (tag_part, sealed) = notice.split_at(TAG_BYTES);
    if tag_part != at.tag {
        return None;
    }
    let (ciphertext, authenticator) = sealed.split_at(PLAINTEXT_BYTES);
    let mut plaintext = Zeroizing::new([0u8; PLAINTEXT_BYTES]);
    plaintext.copy_from_slice(ciphertext);
    cipher(&at.key)
        .decrypt_in_place_detached(
            (&FIXED_NONCE).into(),
            &at.tag,
            plaintext.as_mut(),
            authenticator.into(),
        )
        .ok()?;
    Some(plaintext)
}

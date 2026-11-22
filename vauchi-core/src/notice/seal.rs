// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The seal of a 96-byte notice (ADR-019 amendment 2026-11-22).
//!
//! A notice has no room for a nonce, so the IV is synthetic: HMAC-SHA256
//! over the tag and the plaintext, which is also the authenticator. The
//! plaintext is then encrypted with XChaCha20 under a separate key. A
//! key used twice therefore shows only whether two plaintexts are equal.

use chacha20::XChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::keys::Keyed;
use crate::crypto::HKDF;

pub const TAG_BYTES: usize = 16;
pub const IV_BYTES: usize = 16;
pub const PLAINTEXT_BYTES: usize = 64;
pub const NOTICE_BYTES: usize = TAG_BYTES + IV_BYTES + PLAINTEXT_BYTES;

struct SealKeys {
    mac: Zeroizing<[u8; 32]>,
    encryption: Zeroizing<[u8; 32]>,
}

fn seal_keys(message_key: &[u8; 32]) -> SealKeys {
    let both = HKDF::derive_array::<64>(None, message_key, b"Vauchi_Notice_Seal_v7|siv");
    let (mac_bytes, encryption_bytes) = both.split_at(32);
    let mut mac = Zeroizing::new([0u8; 32]);
    let mut encryption = Zeroizing::new([0u8; 32]);
    mac.copy_from_slice(mac_bytes);
    encryption.copy_from_slice(encryption_bytes);
    SealKeys { mac, encryption }
}

fn synthetic_iv(
    mac_key: &[u8; 32],
    tag: &[u8; TAG_BYTES],
    plaintext: &[u8; PLAINTEXT_BYTES],
) -> [u8; IV_BYTES] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(mac_key)
        .unwrap_or_else(|_| unreachable!("HMAC-SHA256 takes a key of any length"));
    // The associated data's length goes first so that no other split of
    // the same bytes into associated data and plaintext authenticates.
    mac.update(&(TAG_BYTES as u32).to_be_bytes());
    mac.update(tag);
    mac.update(plaintext);
    let digest = mac.finalize().into_bytes();
    let mut iv = [0u8; IV_BYTES];
    for (to, from) in iv.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    iv
}

fn apply_keystream(encryption_key: &[u8; 32], iv: &[u8; IV_BYTES], bytes: &mut [u8]) {
    let mut nonce = [0u8; 24];
    let (iv_part, _zeros) = nonce.split_at_mut(IV_BYTES);
    iv_part.copy_from_slice(iv);
    XChaCha20::new(encryption_key.into(), (&nonce).into()).apply_keystream(bytes);
}

/// Seals a plaintext where `at` says: `tag ‖ iv ‖ ciphertext`.
pub fn seal(at: &Keyed, plaintext: &[u8; PLAINTEXT_BYTES]) -> [u8; NOTICE_BYTES] {
    let keys = seal_keys(&at.key);
    let iv = synthetic_iv(&keys.mac, &at.tag, plaintext);
    let mut notice = [0u8; NOTICE_BYTES];
    let (tag_part, sealed) = notice.split_at_mut(TAG_BYTES);
    let (iv_part, ciphertext) = sealed.split_at_mut(IV_BYTES);
    tag_part.copy_from_slice(&at.tag);
    iv_part.copy_from_slice(&iv);
    ciphertext.copy_from_slice(plaintext);
    apply_keystream(&keys.encryption, &iv, ciphertext);
    notice
}

/// The plaintext, or `None` when the notice is not what `at`'s key
/// sealed at `at`'s tag.
pub fn open(at: &Keyed, notice: &[u8; NOTICE_BYTES]) -> Option<Zeroizing<[u8; PLAINTEXT_BYTES]>> {
    let (tag_part, sealed) = notice.split_at(TAG_BYTES);
    let (iv_part, ciphertext) = sealed.split_at(IV_BYTES);
    let mut iv = [0u8; IV_BYTES];
    iv.copy_from_slice(iv_part);

    let keys = seal_keys(&at.key);
    let mut plaintext = Zeroizing::new([0u8; PLAINTEXT_BYTES]);
    plaintext.copy_from_slice(ciphertext);
    apply_keystream(&keys.encryption, &iv, plaintext.as_mut());

    let expected = synthetic_iv(&keys.mac, &at.tag, &plaintext);
    let authentic = tag_part.ct_eq(&at.tag) & expected.ct_eq(&iv);
    bool::from(authentic).then_some(plaintext)
}

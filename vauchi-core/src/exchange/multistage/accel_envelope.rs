// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AEAD-sealed accelerometer envelope for the multi-stage "shake"
//! co-location signal.
//!
//! The TapHoverShake exchange proves co-location three ways: QR
//! commitment, ultrasonic audio chirp, and — here — an accelerometer
//! magnitude envelope cross-correlated between the two phones. An
//! accelerometer cannot emit, so the only co-location proof is to
//! exchange each peer's envelope over the established multi-stage
//! transport and `cross_correlate` local vs peer.
//!
//! This module owns the *crypto wrapper* around
//! [`shake_protocol::encode_envelope`] (which has no integrity of its
//! own). It realises the security-review acceptance criteria
//! (`investigations/2026-06-03-taphovershake-accel-envelope-security-review.md`):
//!
//! - **F3** — a domain-separated sub-key
//!   `HKDF-Expand(transport_key, "vauchi/multistage/accel-envelope/v1", 32)`,
//!   never `transport_key` directly (which keeps encrypting card chunks).
//! - **F1** — ChaCha20-Poly1305 (the multi-stage transport AEAD), 12-byte
//!   IETF nonce, framing `nonce || ciphertext+tag` (mirrors
//!   `MultiStageSession::transport_encrypt`).
//! - **F2** — `AAD = sender_session_id`. The seal binds the *sender*'s
//!   `session_id`; [`open_envelope`] verifies under the *peer*'s
//!   `session_id`. A reflected own-envelope (sealed with A's sid, fed back
//!   to A which opens under B's sid) **fails AEAD** and is rejected —
//!   defeating the reflection → false-`Confirmed` attack.
//! - **F4** — [`shake_protocol::decode_envelope`] is only ever fed
//!   AEAD-decrypted bytes; the relay only ever sees the sealed blob.

// Slice 1 lands the envelope AEAD core (security-review F1/F2/F3) with its full
// adversarial test suite ahead of the orchestrator caller. Slice 3 wires
// `seal_envelope`/`open_envelope` into the multi-stage SHAK flow and REMOVES
// this attribute, at which point `-D dead-code` re-verifies nothing is unused.
#![allow(dead_code)]

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};

use crate::crypto::kdf::HKDF;
use crate::exchange::shake_protocol::{decode_envelope, encode_envelope};

/// HKDF `info` for the accel-envelope sub-key (ADR-007 domain separation).
///
/// Distinct from the transport key's `b"vauchi-multistage-v1"` so the two
/// keys never collide (security-review F3 / KEY-05).
const ACCEL_ENVELOPE_INFO: &[u8] = b"vauchi/multistage/accel-envelope/v1";

/// ChaCha20-Poly1305 nonce length (12-byte IETF), matching the transport AEAD.
const NONCE_LEN: usize = 12;

/// Poly1305 authentication tag length.
const TAG_LEN: usize = 16;

/// Derive the accel-envelope AEAD sub-key from the transport key (F3).
fn envelope_key(transport_key: &[u8; 32]) -> [u8; 32] {
    let okm = HKDF::expand(transport_key, ACCEL_ENVELOPE_INFO, 32)
        .expect("HKDF expand of a 32-byte sub-key cannot fail");
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm);
    key
}

/// Seal a local accelerometer magnitude envelope for transmission.
///
/// `sender_session_id` is this device's own `session_id`; it is bound as
/// AEAD additional-authenticated-data (F2). The peer opens with
/// [`open_envelope`] passing this device's sid as `peer_session_id`.
///
/// Output framing: `nonce(12) || ciphertext+tag`. The plaintext is
/// [`shake_protocol::encode_envelope`] of `samples`.
pub fn seal_envelope(
    transport_key: &[u8; 32],
    sender_session_id: &[u8; 16],
    samples: &[f32],
) -> Vec<u8> {
    let key = envelope_key(transport_key);
    let plaintext = encode_envelope(samples);
    let nonce_bytes: [u8; NONCE_LEN] = crate::crypto::random_bytes();

    let cipher = ChaCha20Poly1305::new((&key).into());
    let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);
    let payload = Payload {
        msg: &plaintext,
        aad: sender_session_id,
    };
    let ciphertext = cipher
        .encrypt(nonce, payload)
        .expect("ChaCha20-Poly1305 encryption of fresh plaintext cannot fail");

    let mut sealed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    sealed.extend_from_slice(&nonce_bytes);
    sealed.extend_from_slice(&ciphertext);
    sealed
}

/// Open a peer's sealed accelerometer envelope.
///
/// `peer_session_id` is the *expected sender*'s `session_id` (this device's
/// `peer_session_id`), verified as AEAD AAD (F2). Returns `None` on any
/// framing error, AEAD-verification failure (wrong key, wrong sender,
/// tampered ciphertext, reflected own-envelope), or unsupported envelope
/// version. The recovered plaintext is decoded only after AEAD verification
/// succeeds (F4).
pub fn open_envelope(
    transport_key: &[u8; 32],
    peer_session_id: &[u8; 16],
    sealed: &[u8],
) -> Option<Vec<f32>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let key = envelope_key(transport_key);
    let (nonce_bytes, encrypted) = sealed.split_at(NONCE_LEN);

    let cipher = ChaCha20Poly1305::new((&key).into());
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let payload = Payload {
        msg: encrypted,
        aad: peer_session_id,
    };
    let plaintext = cipher.decrypt(nonce, payload).ok()?;
    decode_envelope(&plaintext)
}

// INLINE_TEST_REQUIRED: tests exercise private envelope_key derivation and the
// AEAD seal/open framing; the F2 reflection gate is a security boundary (CC-14).
#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: [u8; 32] = [0x11; 32];
    const SID_A: [u8; 16] = [0xA1; 16];
    const SID_B: [u8; 16] = [0xB2; 16];

    fn sample_envelope() -> Vec<f32> {
        (0..300)
            .map(|i| (i as f32 / 100.0).sin().abs() * 3.0)
            .collect()
    }

    // @internal
    #[test]
    fn seal_open_roundtrip_recovers_samples() {
        let samples = sample_envelope();
        // A seals under its own sid; B opens expecting peer == A.
        let sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        let opened = open_envelope(&KEY_A, &SID_A, &sealed).expect("opens under sender sid");

        assert_eq!(opened.len(), samples.len());
        for (orig, got) in samples.iter().zip(opened.iter()) {
            assert!(
                (orig - got).abs() < 8.0 / 255.0,
                "roundtrip error too large: {orig} vs {got}"
            );
        }
    }

    // @internal
    // CC-14: the F2 reflection gate. An envelope sealed with the sender's sid,
    // reflected back and opened under a *different* (peer) sid, must fail AEAD.
    #[test]
    fn reflected_envelope_under_wrong_sender_sid_is_rejected() {
        let samples = sample_envelope();
        let sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        // A (expecting peer B) receives its own reflected envelope → AAD = B ≠ A.
        assert!(
            open_envelope(&KEY_A, &SID_B, &sealed).is_none(),
            "reflected own-envelope must fail AEAD under the peer sid (F2)"
        );
    }

    // @internal
    #[test]
    fn adversarial_sender_sids_all_rejected() {
        let samples = sample_envelope();
        let sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        // Adversarial AAD candidates an attacker might try: never equal SID_A.
        let mut one_bit_flip = SID_A;
        one_bit_flip[0] ^= 0x01;
        for adversarial in [[0x00u8; 16], [0xFFu8; 16], one_bit_flip] {
            assert!(
                open_envelope(&KEY_A, &adversarial, &sealed).is_none(),
                "AAD {adversarial:?} must not open an envelope sealed under SID_A"
            );
        }
    }

    // @internal
    #[test]
    fn tampered_ciphertext_byte_is_rejected() {
        let samples = sample_envelope();
        let mut sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01; // flip a tag byte
        assert!(open_envelope(&KEY_A, &SID_A, &sealed).is_none());
    }

    // @internal
    #[test]
    fn tampered_nonce_byte_is_rejected() {
        let samples = sample_envelope();
        let mut sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        sealed[0] ^= 0x01; // flip a nonce byte
        assert!(open_envelope(&KEY_A, &SID_A, &sealed).is_none());
    }

    // @internal
    #[test]
    fn truncated_sealed_blob_is_rejected() {
        // Shorter than nonce + tag can carry no authenticated payload.
        for len in [0usize, NONCE_LEN, NONCE_LEN + TAG_LEN - 1] {
            assert!(open_envelope(&KEY_A, &SID_A, &vec![0u8; len]).is_none());
        }
    }

    // @internal
    #[test]
    fn wrong_transport_key_is_rejected() {
        let samples = sample_envelope();
        let sealed = seal_envelope(&KEY_A, &SID_A, &samples);
        let other_key = [0x22u8; 32];
        assert!(open_envelope(&other_key, &SID_A, &sealed).is_none());
    }

    // @internal
    #[test]
    fn envelope_subkey_is_domain_separated_from_transport_key() {
        // The sub-key must differ from the transport key itself (F3): a blob
        // is sealed/opened only with the derived sub-key, never the raw key.
        let derived = envelope_key(&KEY_A);
        assert_ne!(derived, KEY_A, "sub-key must not equal the transport key");
    }

    // @internal
    #[test]
    fn sealed_overhead_fits_single_dense_qr() {
        // 300 samples → 301-byte envelope; + 12 nonce + 16 tag = 329 bytes.
        let sealed = seal_envelope(&KEY_A, &SID_A, &sample_envelope());
        assert_eq!(sealed.len(), 301 + NONCE_LEN + TAG_LEN);
    }

    proptest::proptest! {
        // @internal
        // CC-04: roundtrip over random envelopes and random sids.
        #[test]
        fn roundtrip_property(
            samples in proptest::collection::vec(0.0f32..8.0, 0..400),
            key in proptest::array::uniform32(0u8..),
            sid in proptest::array::uniform16(0u8..),
        ) {
            let sealed = seal_envelope(&key, &sid, &samples);
            let opened = open_envelope(&key, &sid, &sealed)
                .expect("seal then open under the same sid recovers samples");
            proptest::prop_assert_eq!(opened.len(), samples.len());
            for (orig, got) in samples.iter().zip(opened.iter()) {
                proptest::prop_assert!((orig - got).abs() < 8.0 / 255.0);
            }
        }

        // @internal
        // CC-04 + F2: any sid distinct from the sender's never opens.
        #[test]
        fn wrong_sid_never_opens_property(
            samples in proptest::collection::vec(0.0f32..8.0, 1..200),
            sender in proptest::array::uniform16(0u8..),
            peer in proptest::array::uniform16(0u8..),
        ) {
            proptest::prop_assume!(sender != peer);
            let sealed = seal_envelope(&KEY_A, &sender, &samples);
            proptest::prop_assert!(open_envelope(&KEY_A, &peer, &sealed).is_none());
        }
    }
}

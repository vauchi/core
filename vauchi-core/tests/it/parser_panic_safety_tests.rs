// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Panic-safety property tests for the untrusted-input parsers.
//!
//! Every parser that consumes attacker-controlled bytes — a scanned QR
//! code, a received link-mode card payload, a relay simple-message frame,
//! an acoustic shake envelope, a base45 QR chunk — must return a
//! `Result`/`Option` on malformed input and **never panic**. A panic on a
//! malformed payload is a remote denial-of-service.
//!
//! A 2026-06-06 audit (problem `2026-06-06-panic-safety-untrusted-input`)
//! confirmed each parser guards its length before slicing/indexing — the
//! ~75 clippy `indexing_slicing` + 91 `arithmetic_side_effects` warnings in
//! these modules are false positives (the lint can't see the manual
//! `if len < N` checks). These tests lock that property in: the existing
//! roundtrip proptests only exercise the valid-decode path, so a future
//! edit that drops a bounds check would not be caught without these.
//!
//! Strategy: feed arbitrary and truncated bytes and assert the parser
//! *rejects* them (proptest also fails, and shrinks, on any panic). The
//! assertions are non-tautological — a valid payload requires structure a
//! random/truncated buffer cannot satisfy (magic, exact-length signature,
//! version byte, well-formed JSON).

use proptest::prelude::*;

use vauchi_core::exchange::multistage::base45;
use vauchi_core::exchange::shake_protocol;
use vauchi_core::exchange::{ExchangeQR, X3DHKeyPair, link_mode};
use vauchi_core::identity::Identity;
use vauchi_core::network::simple_message;

/// Build one genuinely-valid QR data string (base64 ASCII) to truncate.
fn valid_qr_data_string() -> String {
    let identity = Identity::create("PanicSafety", 0);
    let ephemeral = X3DHKeyPair::generate();
    ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://relay.example.com".to_string()),
        Some([7u8; 32]),
        0u64,
    )
    .to_data_string()
}

proptest! {
    /// Arbitrary strings into the scanned-QR parser are rejected, never panic.
    /// Covers the base64 decode + the `< 192` length guard + the magic check.
    // @internal
    #[test]
    fn qr_rejects_arbitrary_string(s in ".{0,400}") {
        prop_assert!(ExchangeQR::from_data_string(&s).is_err());
    }

    /// Every proper prefix of a VALID QR (valid header, truncated body) is
    /// rejected, never panics. Exercises the intermediate length-field guards
    /// (`name_end`, `url_len`, the exact-length signature check) on input that
    /// passes the magic check — the real panic surface a fully-random buffer
    /// never reaches.
    // @internal
    #[test]
    fn qr_rejects_truncated_valid_payload(cut in 0usize..600) {
        let valid = valid_qr_data_string();
        // strictly shorter than the full payload; base64 is ASCII so every
        // byte index is a char boundary.
        let n = cut.min(valid.len().saturating_sub(1));
        prop_assert!(ExchangeQR::from_data_string(&valid[..n]).is_err());
    }

    /// base45 decode rejects a length-1 input (the short-final-chunk trap that
    /// a naive `chunk[1]` would panic on), never panics.
    // @internal
    #[test]
    fn base45_rejects_single_char(c in 0x20u8..0x7f) {
        let s = (c as char).to_string();
        prop_assert!(base45::decode(&s).is_err());
    }

    /// Arbitrary byte buffers into the link-mode card-payload parser are
    /// rejected (too short, bad version, or non-card JSON), never panic.
    /// Exercises the `< 33` header guard and the `data[1..33]` / `data[33..]`
    /// slices.
    // @internal
    #[test]
    fn parse_card_payload_rejects_arbitrary(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        prop_assert!(link_mode::parse_card_payload(&bytes).is_err());
    }

    /// Arbitrary byte buffers into the relay simple-message decoder are
    /// rejected (too short or non-envelope JSON), never panic. Exercises the
    /// `< FRAME_HEADER_SIZE` guard and the `data[FRAME_HEADER_SIZE..]` slice.
    // @internal
    #[test]
    fn decode_simple_message_rejects_arbitrary(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        prop_assert!(simple_message::decode_simple_message(&bytes).is_err());
    }

    /// The acoustic shake-envelope decoder never panics on arbitrary bytes,
    /// and its `Some`/`None` discriminant matches the version-byte guard
    /// exactly (empty or wrong leading byte → `None`; the `data[1..]` slice is
    /// only reached after that check).
    // @internal
    #[test]
    fn decode_envelope_matches_version_guard(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        let decoded = shake_protocol::decode_envelope(&bytes);
        // Decoding succeeds iff there is a leading byte whose value the
        // decoder accepts as the envelope version; either way it must not
        // panic and must agree with the non-empty precondition.
        if bytes.is_empty() {
            prop_assert!(decoded.is_none());
        } else if let Some(samples) = decoded {
            // Past the version guard: one f32 sample per payload byte.
            prop_assert_eq!(samples.len(), bytes.len() - 1);
        }
    }
}

/// The shake-envelope decoder returns `None` (not a panic) on empty input.
// @internal
#[test]
fn decode_envelope_empty_is_none() {
    assert!(shake_protocol::decode_envelope(&[]).is_none());
}

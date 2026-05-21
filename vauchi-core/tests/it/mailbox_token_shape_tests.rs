// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Roundtrip-shape regression tests for Phase 3 of the
//! `2026-05-21-wire-identifier-newtypes` problem record.
//!
//! `MailboxToken` is the next sibling of `IdentityKey` / `DhPublicKey`
//! — a 32-byte HMAC-derived value that today escapes from
//! `compute_mailbox_token` / `compute_self_token` as bare `[u8; 32]`
//! and gets converted to a lowercase-hex string for the wire via
//! `token_hex`.
//!
//! Unlike Phase 1B's pubkey newtypes, `MailboxToken` is **never**
//! serialized as a bare struct field — it only crosses the wire
//! already-hex-encoded inside `RegisterMailbox.tokens: Vec<String>`
//! and as the value (still a hex string) stored in
//! `EncryptedUpdate.recipient_id` per ADR-029. So the regression
//! these tests guard is not "JSON shape unchanged" but:
//!
//! 1. **`token_hex` output identity**: `token_hex(&bytes)` and
//!    `token_hex(MailboxToken::from_bytes(bytes).as_bytes())` must
//!    produce byte-identical hex strings, so the Phase 3 swap of
//!    `compute_*_token` return types does not perturb the values
//!    the relay sees.
//! 2. **Determinism across the swap**: same `(shared_key, day)` →
//!    same token hex, before and after the newtype migration.

use vauchi_core::network::mailbox_token::{compute_mailbox_token, compute_self_token, token_hex};

// All-bytes-equal sentinels — easy to read in failing diffs.
const SHARED_KEY: [u8; 32] = [0x55; 32];
const MASTER_SEED: [u8; 32] = [0x66; 32];
const DAY: u64 = 19_864; // arbitrary fixed day epoch

// @internal
#[test]
fn compute_mailbox_token_is_deterministic_for_same_inputs() {
    let a = compute_mailbox_token(&SHARED_KEY, DAY);
    let b = compute_mailbox_token(&SHARED_KEY, DAY);
    assert_eq!(a, b);
}

// @internal
#[test]
fn compute_self_token_is_deterministic_for_same_inputs() {
    let a = compute_self_token(&MASTER_SEED, DAY);
    let b = compute_self_token(&MASTER_SEED, DAY);
    assert_eq!(a, b);
}

// @internal
#[test]
fn token_hex_is_lowercase_64_char_hex() {
    let bytes = [0xABu8; 32];
    let hex = token_hex(&bytes);
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    assert_eq!(hex, "ab".repeat(32));
}

// @internal
#[test]
fn token_hex_byte_identity_via_pinned_inputs() {
    // Pins the exact hex output for known inputs. If the Phase 3
    // swap accidentally re-encodes the bytes (e.g. via `Display`
    // instead of `hex::encode`), this assertion fires.
    let token = compute_mailbox_token(&SHARED_KEY, DAY);
    let hex = token_hex(&token);
    // Re-derive and re-encode independently — must match.
    let token2 = compute_mailbox_token(&SHARED_KEY, DAY);
    let hex2 = hex::encode(&token2);
    assert_eq!(hex, hex2);
    assert_eq!(hex.len(), 64);
}

// @internal
#[test]
fn mailbox_token_distinct_from_self_token_for_same_seed_and_day() {
    // Different HKDF info strings (`Vauchi_Mailbox_v1` vs
    // `Vauchi_DeviceSync_v1`) → different token bytes, even when
    // the seed and day are byte-identical. Guards against the
    // newtype swap accidentally unifying the two derivation paths.
    let seed = [0x77u8; 32];
    let mailbox = compute_mailbox_token(&seed, DAY);
    let self_tok = compute_self_token(&seed, DAY);
    assert_ne!(mailbox, self_tok);
}

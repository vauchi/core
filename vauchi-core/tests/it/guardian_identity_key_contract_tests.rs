// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proves the real-identity guardian seal/open contract through the public
//! guardian API (Candidate 1, problem
//! `2026-07-13-mobile-guardian-backup-integration`): a key shard sealed to a
//! guardian's advertised Ed25519 signing key is openable by *that* identity's
//! derived X25519 secret, and by no other.
//!
//! Before the fix the opener derived an unrelated HKDF exchange secret, so a
//! real identity could never open its own entry — the pre-fix tests passed
//! only because they generated synthetic matching X25519 pairs, never
//! exercising the identity boundary used in production.

use proptest::prelude::*;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::{
    BackupKey, BackupKeyShard, GuardianBackupMetadata, KeyShardConfig, open_share_for_guardian,
    seal_share_for_guardian, split_backup_key,
};

fn a_shard() -> BackupKeyShard {
    let key = BackupKey::generate();
    let config = KeyShardConfig::new(2, 3).expect("valid config");
    split_backup_key(&key, GuardianBackupMetadata::generate(config))
        .expect("split succeeds")
        .swap_remove(0)
}

// @internal
#[test]
fn identity_opens_share_sealed_to_its_signing_key() {
    let guardian = SigningKeyPair::from_seed(&[9u8; 32]);
    let recipient = guardian.public_key().to_x25519().expect("valid point");
    let shard = a_shard();

    let sealed = seal_share_for_guardian(&shard, &recipient).expect("seal succeeds");
    let opened =
        open_share_for_guardian(&sealed, &guardian.to_x25519_secret()).expect("identity opens it");

    assert_eq!(
        opened, shard,
        "an identity must open a share sealed to its advertised signing key"
    );
}

// @internal
#[test]
fn a_different_guardian_cannot_open_the_share() {
    let guardian = SigningKeyPair::from_seed(&[1u8; 32]);
    let attacker = SigningKeyPair::from_seed(&[2u8; 32]);
    let recipient = guardian.public_key().to_x25519().expect("valid point");

    let sealed = seal_share_for_guardian(&a_shard(), &recipient).expect("seal succeeds");

    assert!(
        open_share_for_guardian(&sealed, &attacker.to_x25519_secret()).is_err(),
        "a non-recipient identity must not open the share"
    );
}

proptest! {
    // @internal
    #[test]
    fn seal_open_roundtrips_for_any_identity(seed: [u8; 32]) {
        let guardian = SigningKeyPair::from_seed(&seed);
        let recipient = guardian.public_key().to_x25519().expect("valid point");
        let shard = a_shard();

        let sealed = seal_share_for_guardian(&shard, &recipient).expect("seal succeeds");
        let opened =
            open_share_for_guardian(&sealed, &guardian.to_x25519_secret()).expect("opens");

        prop_assert_eq!(opened, shard);
    }
}

/// Regression guard for a deliberate design choice: the derivation is NOT
/// replaced by ed25519-dalek's `to_scalar_bytes()`. That method returns the
/// signing scalar reduced mod L, whereas an X25519 static secret must be the
/// clamped expanded scalar `clamp(SHA-512(seed)[0..32])`, which is always
/// >= 2^254 > L. The raw bytes therefore always differ, and
/// `StaticSecret::from(to_scalar_bytes())` would re-clamp to the wrong value —
/// so it must never be substituted. Correctness of our construction is pinned
/// instead by `to_x25519_secret_known_answer` (external libsodium reference)
/// and the `x25519_secret_public_matches_recipient_for_any_seed` proptest
/// (derived public key == `VerifyingKey::to_montgomery`).
// @internal
#[test]
fn to_scalar_bytes_is_not_the_x25519_secret() {
    let ours = SigningKeyPair::from_seed(&[1u8; 32])
        .to_x25519_secret()
        .to_bytes();
    let dalek = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]).to_scalar_bytes();
    assert_ne!(
        ours, dalek,
        "to_scalar_bytes is the reduced scalar, not the clamped x25519 secret"
    );
}

// @internal
/// Known-answer test pinning the libsodium `crypto_sign_ed25519_sk_to_curve25519`
/// output for seed `[1u8; 32]`. `expected` was computed by an independent Python
/// reference (`clamp(SHA-512(seed)[0..32])`), so this catches a silent change in
/// both the manual path and the dalek path at once.
#[test]
fn to_x25519_secret_known_answer() {
    let secret = SigningKeyPair::from_seed(&[1u8; 32]).to_x25519_secret();
    let expected: [u8; 32] = [
        0x58, 0xe8, 0x6e, 0xfb, 0x75, 0xfa, 0x4e, 0x2c, 0x41, 0x0f, 0x46, 0xe1, 0x6d, 0xe9, 0xf6,
        0xac, 0xae, 0x1a, 0x17, 0x03, 0x52, 0x86, 0x51, 0xb6, 0x9b, 0xc1, 0x76, 0xc0, 0x88, 0xbe,
        0xf3, 0x6e,
    ];
    assert_eq!(secret.to_bytes(), expected);
}

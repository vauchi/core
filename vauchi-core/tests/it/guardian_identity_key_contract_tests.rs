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
    BackupKey, BackupKeyShard, KeyShardConfig, open_share_for_guardian, seal_share_for_guardian,
    split_backup_key,
};

fn a_shard() -> BackupKeyShard {
    let key = BackupKey::generate();
    split_backup_key(&key, KeyShardConfig::new(2, 3).expect("valid config"))
        .expect("split succeeds")
        .swap_remove(0)
}

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

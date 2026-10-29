// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The purge an identity deletion sends is signed when it is sent, in the
//! format the relay verifies (vauchi/private#899, #911).

use vauchi_core::api::PreSignedPurgeRequest;
use vauchi_core::crypto::{PublicKey as CryptoPublicKey, Signature as CryptoSignature};
use vauchi_core::identity::Identity;

/// What the relay's `verify_purge_ed25519` checks, minus its clock.
fn relay_verifies(purge: &PreSignedPurgeRequest) -> bool {
    let mut message = Vec::with_capacity(32 + 32 + 8);
    message.extend_from_slice(&purge.public_key);
    message.extend_from_slice(&purge.purge_token);
    message.extend_from_slice(&purge.timestamp.to_be_bytes());
    let signature: [u8; 64] = purge.signature.clone().try_into().expect("64 bytes");
    CryptoPublicKey::from_bytes(purge.public_key)
        .verify(&message, &CryptoSignature::from_bytes(signature))
}

// @scenario: pre_signed_operations :: Relay can verify purge request signature
#[test]
fn a_purge_signed_now_verifies_in_the_relays_format_with_my_key() {
    let identity = Identity::create("Grace", 0);

    let purge = PreSignedPurgeRequest::sign_now(&identity, 1_700_000_000);

    assert_eq!(purge.public_key, *identity.signing_public_key());
    assert_eq!(purge.timestamp, 1_700_000_000);
    assert!(relay_verifies(&purge));
}

// @scenario: pre_signed_operations :: Each purge carries its own one-time token
#[test]
fn each_purge_carries_its_own_token() {
    let identity = Identity::create("Frank", 0);

    let first = PreSignedPurgeRequest::sign_now(&identity, 1_700_000_000);
    let second = PreSignedPurgeRequest::sign_now(&identity, 1_700_000_000);

    assert_ne!(first.purge_token, second.purge_token);
}

// @scenario: pre_signed_operations :: Tampered pre-signed message is rejected
#[test]
fn a_purge_with_a_changed_token_or_timestamp_fails_verification() {
    let identity = Identity::create("Mallory", 0);
    let purge = PreSignedPurgeRequest::sign_now(&identity, 1_700_000_000);

    let mut token_changed = purge.clone();
    token_changed.purge_token[0] ^= 0x01;
    let mut timestamp_changed = purge.clone();
    timestamp_changed.timestamp += 1;

    assert_eq!(
        (
            relay_verifies(&token_changed),
            relay_verifies(&timestamp_changed)
        ),
        (false, false)
    );
}

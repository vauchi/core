// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the guardian token end-to-end workflow.
//!
//! Tests the full cycle: create tokens, encrypt to guardians, decrypt,
//! verify tokens — without network calls.
//!
//! Traces to: features/contact_recovery.feature

use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::recovery::sealed_box;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

// Test helper: same computation as compute_guardian_hash in recovery.rs
fn compute_guardian_hash_for_test(designator_pk: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(designator_pk);
    hasher.update(b"guardians");
    hex::encode(hasher.finalize())
}

/// Tests the full guardian token lifecycle:
/// designator creates token → encrypts to guardian → guardian decrypts → verifies.
// @scenario: contact_recovery :: Full lifecycle of guardian token creation, encryption, decryption, and verification
#[test]
fn test_guardian_token_seal_unseal_verify_lifecycle() {
    // Designator (Alice) creates her identity
    let alice_signing = SigningKeyPair::generate();

    // Guardian (Bob) has an X25519 keypair
    let bob_x25519_secret = StaticSecret::random_from_rng(OsRng);
    let bob_x25519_public = X25519PublicKey::from(&bob_x25519_secret);

    // Bob also has an Ed25519 identity (for the token)
    let bob_signing = SigningKeyPair::generate();

    // Alice creates a guardian token for Bob
    let token = GuardianToken::create(&alice_signing, bob_signing.public_key());
    assert!(token.verify(), "freshly created token must verify");

    // Alice encrypts the token to Bob's X25519 key
    let token_bytes = token.to_bytes();
    let sealed = sealed_box::seal(&token_bytes, &bob_x25519_public);

    // Bob decrypts the entry
    let decrypted = sealed_box::open(&sealed, &bob_x25519_secret)
        .expect("Bob must be able to decrypt his own entry");
    let restored_token =
        GuardianToken::from_bytes(&decrypted).expect("decrypted bytes must deserialize to token");

    // Bob verifies the token
    assert!(restored_token.verify(), "restored token must verify");
    assert_eq!(
        restored_token.designator_pk(),
        alice_signing.public_key().as_bytes(),
        "designator_pk must match Alice's key"
    );
    assert_eq!(
        restored_token.guardian_pk(),
        bob_signing.public_key().as_bytes(),
        "guardian_pk must match Bob's key"
    );
}

/// Tests that a non-guardian cannot decrypt any entry.
// @scenario: contact_recovery :: Non-guardian cannot decrypt sealed guardian entry
#[test]
fn test_non_guardian_cannot_decrypt_sealed_entry() {
    let alice_signing = SigningKeyPair::generate();
    let bob_signing = SigningKeyPair::generate();
    let bob_x25519_secret = StaticSecret::random_from_rng(OsRng);
    let bob_x25519_public = X25519PublicKey::from(&bob_x25519_secret);

    // Eve is not a guardian
    let eve_x25519_secret = StaticSecret::random_from_rng(OsRng);

    let token = GuardianToken::create(&alice_signing, bob_signing.public_key());
    let sealed = sealed_box::seal(&token.to_bytes(), &bob_x25519_public);

    // Eve tries to decrypt Bob's entry
    let result = sealed_box::open(&sealed, &eve_x25519_secret);
    assert!(
        result.is_err(),
        "non-guardian should not be able to decrypt"
    );
}

/// Tests deterministic guardian hash computation.
// @scenario: contact_recovery :: Guardian hash computation is deterministic for the same key
#[test]
fn test_guardian_hash_is_deterministic() {
    let pk = [0xABu8; 32];
    let hash1 = compute_guardian_hash_for_test(&pk);
    let hash2 = compute_guardian_hash_for_test(&pk);
    assert_eq!(hash1, hash2, "same key must always produce the same hash");
    assert_eq!(hash1.len(), 64, "SHA-256 hex-encoded must be 64 chars");
}

/// Tests that different designator keys produce different hashes.
// @scenario: contact_recovery :: Different designator keys produce different guardian hashes
#[test]
fn test_guardian_hash_differs_per_designator() {
    let pk1 = [0x01u8; 32];
    let pk2 = [0x02u8; 32];
    let hash1 = compute_guardian_hash_for_test(&pk1);
    let hash2 = compute_guardian_hash_for_test(&pk2);
    assert_ne!(hash1, hash2, "different keys must produce different hashes");
}

/// Tests Ed25519 to X25519 key conversion consistency.
// @scenario: contact_recovery :: Ed25519 to X25519 key conversion is deterministic
#[test]
fn test_ed25519_to_x25519_conversion_is_deterministic() {
    // Create an Ed25519 keypair
    let signing = SigningKeyPair::generate();
    let ed25519_pk = signing.public_key();

    // Convert to X25519
    let verifying_key =
        ed25519_dalek::VerifyingKey::from_bytes(ed25519_pk.as_bytes()).expect("valid Ed25519 key");
    let montgomery = verifying_key.to_montgomery();
    let x25519_pk = X25519PublicKey::from(montgomery.to_bytes());

    // The conversion must be deterministic
    let montgomery2 = verifying_key.to_montgomery();
    let x25519_pk2 = X25519PublicKey::from(montgomery2.to_bytes());
    assert_eq!(
        x25519_pk.as_bytes(),
        x25519_pk2.as_bytes(),
        "Ed25519→X25519 conversion must be deterministic"
    );
}

/// Tests multiple guardians: each can only decrypt their own entry.
// @scenario: contact_recovery :: Each guardian can only decrypt their own sealed entry
#[test]
fn test_multiple_guardians_isolation() {
    let alice = SigningKeyPair::generate();

    // Three guardians
    let bob_signing = SigningKeyPair::generate();
    let bob_secret = StaticSecret::random_from_rng(OsRng);
    let bob_public = X25519PublicKey::from(&bob_secret);

    let charlie_signing = SigningKeyPair::generate();
    let charlie_secret = StaticSecret::random_from_rng(OsRng);
    let charlie_public = X25519PublicKey::from(&charlie_secret);

    let dave_signing = SigningKeyPair::generate();
    let dave_secret = StaticSecret::random_from_rng(OsRng);
    let dave_public = X25519PublicKey::from(&dave_secret);

    // Alice creates and seals one entry per guardian
    let sealed_entries: Vec<Vec<u8>> = [
        (bob_signing.public_key(), &bob_public),
        (charlie_signing.public_key(), &charlie_public),
        (dave_signing.public_key(), &dave_public),
    ]
    .iter()
    .map(|(guardian_pk, x25519_pk)| {
        let token = GuardianToken::create(&alice, guardian_pk.clone());
        sealed_box::seal(&token.to_bytes(), x25519_pk)
    })
    .collect();

    // Bob can decrypt exactly one entry (his own)
    let bob_decrypted: Vec<_> = sealed_entries
        .iter()
        .filter_map(|entry| sealed_box::open(entry, &bob_secret).ok())
        .collect();
    assert_eq!(bob_decrypted.len(), 1, "Bob must decrypt exactly one entry");

    // Charlie can decrypt exactly one entry (his own)
    let charlie_decrypted: Vec<_> = sealed_entries
        .iter()
        .filter_map(|entry| sealed_box::open(entry, &charlie_secret).ok())
        .collect();
    assert_eq!(
        charlie_decrypted.len(),
        1,
        "Charlie must decrypt exactly one entry"
    );

    // Dave can decrypt exactly one entry (his own)
    let dave_decrypted: Vec<_> = sealed_entries
        .iter()
        .filter_map(|entry| sealed_box::open(entry, &dave_secret).ok())
        .collect();
    assert_eq!(
        dave_decrypted.len(),
        1,
        "Dave must decrypt exactly one entry"
    );

    // Each decrypted entry contains the right guardian_pk
    let bob_token =
        GuardianToken::from_bytes(&bob_decrypted[0]).expect("Bob's decrypted bytes must parse");
    assert_eq!(
        bob_token.guardian_pk(),
        bob_signing.public_key().as_bytes(),
        "Bob's token must identify Bob as guardian"
    );
    assert!(bob_token.verify(), "Bob's token must verify");

    let charlie_token = GuardianToken::from_bytes(&charlie_decrypted[0])
        .expect("Charlie's decrypted bytes must parse");
    assert_eq!(
        charlie_token.guardian_pk(),
        charlie_signing.public_key().as_bytes(),
        "Charlie's token must identify Charlie as guardian"
    );
    assert!(charlie_token.verify(), "Charlie's token must verify");

    let dave_token =
        GuardianToken::from_bytes(&dave_decrypted[0]).expect("Dave's decrypted bytes must parse");
    assert_eq!(
        dave_token.guardian_pk(),
        dave_signing.public_key().as_bytes(),
        "Dave's token must identify Dave as guardian"
    );
    assert!(dave_token.verify(), "Dave's token must verify");
}

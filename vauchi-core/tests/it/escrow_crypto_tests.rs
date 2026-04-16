// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for escrow HKDF key derivation and card encryption.

use vauchi_core::exchange::escrow::{EscrowKeys, EscrowRole};

/// Fixed shared secret for deterministic tests.
fn test_shared_secret() -> Vec<u8> {
    // 32 bytes — typical X25519 output
    (0..32).collect()
}

fn alt_shared_secret() -> Vec<u8> {
    vec![0xff; 32]
}

// ================================================================
// Deterministic derivation
// ================================================================

// @internal
#[test]
fn derive_produces_64_char_hex_hashes() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    assert_eq!(keys.gate_hash.len(), 64);
    assert_eq!(keys.our_slot.len(), 64);
    assert_eq!(keys.their_slot.len(), 64);
    assert!(keys.gate_hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(keys.our_slot.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(keys.their_slot.chars().all(|c| c.is_ascii_hexdigit()));
}

// @internal
#[test]
fn same_secret_produces_same_gate_hash() {
    let init = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let resp = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Responder);
    assert_eq!(
        init.gate_hash, resp.gate_hash,
        "Both roles must derive the same gate_hash"
    );
}

// @internal
#[test]
fn roles_swap_slot_assignment() {
    let init = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let resp = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Responder);

    assert_eq!(
        init.our_slot, resp.their_slot,
        "Initiator's our_slot = Responder's their_slot"
    );
    assert_eq!(
        init.their_slot, resp.our_slot,
        "Initiator's their_slot = Responder's our_slot"
    );
}

// @internal
#[test]
fn derivation_is_deterministic() {
    let a = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let b = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    assert_eq!(a.gate_hash, b.gate_hash);
    assert_eq!(a.our_slot, b.our_slot);
    assert_eq!(a.their_slot, b.their_slot);
}

// ================================================================
// Domain separation — security critical
// ================================================================

// @internal
#[test]
fn gate_slot_init_slot_resp_are_all_distinct() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    assert_ne!(keys.gate_hash, keys.our_slot, "gate ≠ our_slot");
    assert_ne!(keys.gate_hash, keys.their_slot, "gate ≠ their_slot");
    assert_ne!(keys.our_slot, keys.their_slot, "our_slot ≠ their_slot");
}

// @internal
#[test]
fn different_secrets_produce_different_values() {
    let a = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let b = EscrowKeys::derive(&alt_shared_secret(), EscrowRole::Initiator);
    assert_ne!(a.gate_hash, b.gate_hash);
    assert_ne!(a.our_slot, b.our_slot);
    assert_ne!(a.their_slot, b.their_slot);
}

// ================================================================
// Card encryption roundtrip
// ================================================================

// @internal
#[test]
fn encrypt_decrypt_roundtrip() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let plaintext = b"Alice's contact card data";

    let ciphertext = keys.encrypt_card(plaintext).unwrap();
    assert_ne!(
        &ciphertext, plaintext,
        "ciphertext must differ from plaintext"
    );

    let decrypted = keys.decrypt_card(&ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

// @internal
#[test]
fn both_roles_can_decrypt_each_others_cards() {
    let init = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let resp = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Responder);

    let alice_card = b"Alice's card";
    let bob_card = b"Bob's card";

    let alice_ct = init.encrypt_card(alice_card).unwrap();
    let bob_ct = resp.encrypt_card(bob_card).unwrap();

    // Responder decrypts initiator's card
    let decrypted_alice = resp.decrypt_card(&alice_ct).unwrap();
    assert_eq!(decrypted_alice, alice_card);

    // Initiator decrypts responder's card
    let decrypted_bob = init.decrypt_card(&bob_ct).unwrap();
    assert_eq!(decrypted_bob, bob_card);
}

// @internal
#[test]
fn wrong_key_fails_decryption() {
    let keys_a = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let keys_b = EscrowKeys::derive(&alt_shared_secret(), EscrowRole::Initiator);

    let ct = keys_a.encrypt_card(b"secret").unwrap();
    assert!(
        keys_b.decrypt_card(&ct).is_err(),
        "Decryption with wrong key must fail"
    );
}

// @internal
#[test]
fn tampered_ciphertext_fails() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let mut ct = keys.encrypt_card(b"secret data").unwrap();

    // Flip a byte in the ciphertext body (past nonce)
    let last = ct.len() - 1;
    ct[last] ^= 0xff;

    assert!(
        keys.decrypt_card(&ct).is_err(),
        "Tampered ciphertext must fail authentication"
    );
}

// ================================================================
// Adversarial inputs
// ================================================================

// @internal
#[test]
fn empty_plaintext_roundtrips() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let ct = keys.encrypt_card(b"").unwrap();
    let pt = keys.decrypt_card(&ct).unwrap();
    assert!(pt.is_empty());
}

// @internal
#[test]
fn large_plaintext_roundtrips() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let big = vec![0xAB; 60_000]; // near 64 KiB escrow limit
    let ct = keys.encrypt_card(&big).unwrap();
    let pt = keys.decrypt_card(&ct).unwrap();
    assert_eq!(pt, big);
}

// @internal
#[test]
fn truncated_ciphertext_fails() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let ct = keys.encrypt_card(b"data").unwrap();

    // Truncate to just the tag + partial nonce
    assert!(keys.decrypt_card(&ct[..5]).is_err());
}

// @internal
#[test]
fn flipped_nonce_byte_fails() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let mut ct = keys.encrypt_card(b"data").unwrap();

    // Flip byte in the nonce region (byte 1, after algorithm tag)
    ct[1] ^= 0xff;

    assert!(
        keys.decrypt_card(&ct).is_err(),
        "Flipped nonce must cause decryption failure"
    );
}

// @internal
#[test]
fn each_encryption_produces_different_ciphertext() {
    let keys = EscrowKeys::derive(&test_shared_secret(), EscrowRole::Initiator);
    let ct1 = keys.encrypt_card(b"same").unwrap();
    let ct2 = keys.encrypt_card(b"same").unwrap();
    assert_ne!(ct1, ct2, "Random nonce must produce different ciphertext");
}

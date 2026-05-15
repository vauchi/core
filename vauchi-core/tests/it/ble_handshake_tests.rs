// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Handshake Session Tests
//!
//! Tests for the 4-phase BLE encrypted handshake protocol.
//! Covers crypto primitives, state machine transitions, commitment scheme,
//! and error paths.

use sha2::{Digest, Sha256};
use vauchi_core::ExchangeError;
use vauchi_core::crypto::encryption::{self, SymmetricKey};
use vauchi_core::crypto::kdf::HKDF;
use vauchi_core::exchange::{
    BLE_HANDSHAKE_VERSION, BleCardPayload, BleHandshakeSession, BleHandshakeState, X3DHKeyPair,
};
use vauchi_core::identity::Identity;

fn make_test_identity() -> Identity {
    Identity::create("Test", 0)
}

fn make_test_card(identity: &Identity, name: &str) -> BleCardPayload {
    let exchange_keys = X3DHKeyPair::generate();
    BleCardPayload::new(
        *identity.signing_public_key(),
        name.to_string(),
        *exchange_keys.public_key(),
        vec![("email".into(), "test@example.com".into())],
        None,
    )
}

// ============================================================
// Crypto Primitive Tests
// ============================================================

// @scenario: ble_exchange :: X25519 shared secret symmetry
#[test]
fn test_x25519_shared_secret_symmetry() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_shared = alice.diffie_hellman(bob.public_key()).unwrap();
    let bob_shared = bob.diffie_hellman(alice.public_key()).unwrap();

    assert_eq!(
        alice_shared, bob_shared,
        "DH shared secrets must be symmetric: A.dh(B.pub) == B.dh(A.pub)"
    );
}

// @scenario: ble_exchange :: HKDF produces deterministic output
#[test]
fn test_hkdf_deterministic_output() {
    let ikm = [42u8; 32];
    let salt = [1u8; 32];
    let info = b"test-info";

    let key1 = HKDF::derive_key(Some(&salt), &ikm, info);
    let key2 = HKDF::derive_key(Some(&salt), &ikm, info);

    assert_eq!(
        *key1, *key2,
        "HKDF must produce identical output for identical inputs"
    );
}

// @scenario: ble_exchange :: XChaCha20-Poly1305 encryption roundtrip
#[test]
fn test_xchacha20_roundtrip() {
    let key = SymmetricKey::generate();
    let plaintext = b"hello BLE exchange";
    let aad = b"sender||receiver||timestamp";

    let ciphertext =
        encryption::encrypt_with_ad(&key, plaintext, aad).expect("encryption should succeed");
    let decrypted =
        encryption::decrypt_with_ad(&key, &ciphertext, aad).expect("decryption should succeed");

    assert_eq!(
        decrypted, plaintext,
        "Decrypted plaintext must match original"
    );
}

// @scenario: ble_exchange :: Tampered ciphertext fails AEAD decryption
#[test]
fn test_tampered_ciphertext_fails_decryption() {
    let key = SymmetricKey::generate();
    let plaintext = b"sensitive card data";
    let aad = b"context-binding";

    let mut ciphertext =
        encryption::encrypt_with_ad(&key, plaintext, aad).expect("encryption should succeed");

    // Flip a byte in the ciphertext body (past the algorithm tag + nonce)
    let flip_idx = ciphertext.len() - 5;
    ciphertext[flip_idx] ^= 0xFF;

    let result = encryption::decrypt_with_ad(&key, &ciphertext, aad);
    assert!(
        result.is_err(),
        "Tampered ciphertext must fail AEAD authentication"
    );
}

// @scenario: ble_exchange :: Wrong AAD fails AEAD decryption
#[test]
fn test_tampered_aad_fails_decryption() {
    let key = SymmetricKey::generate();
    let plaintext = b"card payload";
    let aad = b"correct-aad";

    let ciphertext =
        encryption::encrypt_with_ad(&key, plaintext, aad).expect("encryption should succeed");

    let wrong_aad = b"wrong-aad";
    let result = encryption::decrypt_with_ad(&key, &ciphertext, wrong_aad);
    assert!(result.is_err(), "Wrong AAD must fail AEAD authentication");
}

// @scenario: ble_exchange :: Symmetric key is zeroized on drop
#[test]
fn test_zeroize_on_drop() {
    // Documents the ZeroizeOnDrop contract: SymmetricKey derives ZeroizeOnDrop.
    // We cannot directly observe zeroization in safe Rust, but we verify
    // the type is constructed and dropped without panic.
    let key = SymmetricKey::generate();
    let key_bytes = *key.as_bytes();
    assert!(
        key_bytes.iter().any(|&b| b != 0),
        "Generated key must not be all zeros"
    );
    drop(key);
    // After drop, key material would be zeroized by ZeroizeOnDrop.
    // The contract is enforced by the derive macro; this test documents it.
}

// ============================================================
// State Machine: Construction & Initial State
// ============================================================

// @scenario: ble_exchange :: Initiator starts in Idle state
#[test]
fn test_new_initiator_starts_idle() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert!(
        matches!(session.state(), BleHandshakeState::Idle),
        "New initiator session must start in Idle state"
    );
}

// @scenario: ble_exchange :: Responder starts in Idle state
#[test]
fn test_new_responder_starts_idle() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Bob");
    let session = BleHandshakeSession::new_responder(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    assert!(
        matches!(session.state(), BleHandshakeState::Idle),
        "New responder session must start in Idle state"
    );
}

// ============================================================
// Phase 1: Key Offer
// ============================================================

// @scenario: ble_exchange :: Key offer has correct format
#[test]
fn test_create_key_offer_format() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let mut session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = session
        .create_key_offer()
        .expect("key offer should succeed");

    // version(1) + identity_pub(32) + ephemeral_pub(32) + nonce(16) + timestamp(8) = 89
    assert_eq!(offer.len(), 121, "v2 KeyOffer must be exactly 121 bytes");
    assert_eq!(
        offer[0], BLE_HANDSHAKE_VERSION,
        "First byte must be version tag"
    );

    assert!(
        matches!(session.state(), BleHandshakeState::KeyOfferSent { .. }),
        "State must transition to KeyOfferSent"
    );
}

// @scenario: ble_exchange :: Double key offer is rejected
#[test]
fn test_double_key_offer_rejected() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let mut session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    session.create_key_offer().expect("first offer");
    let result = session.create_key_offer();
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "Second key offer must be rejected"
    );
}

// ============================================================
// Phase 2: Key Ack (Responder processes offer)
// ============================================================

// @scenario: ble_exchange :: Responder processes key offer
#[test]
fn test_responder_processes_key_offer() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = alice.create_key_offer().expect("key offer");

    let (ack_bytes, encrypted_card) = bob
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process key offer");

    // v2 KeyAck: version(1) + identity(32) + exchange(32) + ephemeral(32) + nonce(16) + commitment(32) = 145
    assert_eq!(ack_bytes.len(), 145, "v2 KeyAck must be exactly 145 bytes");
    assert_eq!(
        ack_bytes[0], BLE_HANDSHAKE_VERSION,
        "KeyAck version must match"
    );

    // Commitment is SHA-256 of encrypted card (at offset 113..145 in v2)
    let expected_commitment = Sha256::digest(&encrypted_card);
    assert_eq!(
        &ack_bytes[113..145],
        &expected_commitment[..],
        "Commitment in KeyAck must be SHA-256(encrypted_card)"
    );

    assert!(
        !encrypted_card.is_empty(),
        "Encrypted card must not be empty"
    );
}

// @scenario: ble_exchange :: Responder rejects key offer with invalid version
#[test]
fn test_responder_rejects_invalid_version() {
    let bob_id = make_test_identity();
    let bob_card = make_test_card(&bob_id, "Bob");
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let mut bad_offer = vec![0x99u8]; // Wrong version
    bad_offer.extend_from_slice(&[0u8; 120]); // Pad to 121 bytes (v2 size)

    let result = bob.process_key_offer(
        &bad_offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(result.is_err(), "Invalid version must be rejected");
}

// @scenario: ble_exchange :: Responder rejects truncated key offer
#[test]
fn test_responder_rejects_truncated_offer() {
    let bob_id = make_test_identity();
    let bob_card = make_test_card(&bob_id, "Bob");
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let short_offer = vec![BLE_HANDSHAKE_VERSION; 10]; // Too short
    let result = bob.process_key_offer(
        &short_offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(result.is_err(), "Truncated offer must be rejected");
}

// ============================================================
// Phase 2 (Initiator): Process Key Ack
// ============================================================

// @scenario: ble_exchange :: Initiator processes key acknowledgment
#[test]
fn test_initiator_processes_key_ack() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = alice.create_key_offer().expect("key offer");
    let (ack_bytes, bob_encrypted_card) = bob
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process offer");

    let (commitment, alice_encrypted_card) = alice
        .process_key_ack(&ack_bytes, &bob_encrypted_card)
        .expect("process key ack");

    assert_eq!(
        commitment.len(),
        32,
        "Commitment must be 32 bytes (SHA-256)"
    );
    assert!(
        !alice_encrypted_card.is_empty(),
        "Alice's encrypted card must not be empty"
    );

    // Commitment must be SHA-256 of Alice's encrypted card
    let expected = Sha256::digest(&alice_encrypted_card);
    assert_eq!(
        commitment.as_slice(),
        &expected[..],
        "Commitment must match SHA-256(encrypted_card)"
    );
}

// @scenario: ble_exchange :: Initiator rejects ack in wrong state
#[test]
fn test_initiator_rejects_ack_in_wrong_state() {
    let alice_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    // Haven't sent key offer yet
    let result = alice.process_key_ack(&[0u8; 113], &[0u8; 64]);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "process_key_ack in Idle state must fail"
    );
}

// ============================================================
// Phase 3: Committed Payload
// ============================================================

// @scenario: ble_exchange :: Responder processes committed payload
#[test]
fn test_responder_processes_committed_payload() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    // Phase 1
    let offer = alice.create_key_offer().expect("key offer");
    // Phase 2 (responder)
    let (ack_bytes, bob_encrypted) = bob
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process offer");
    // Phase 2 (initiator)
    let (commitment, alice_encrypted) = alice
        .process_key_ack(&ack_bytes, &bob_encrypted)
        .expect("process ack");

    // Phase 3: Bob processes Alice's committed payload
    let reveal = bob
        .process_committed_payload(&commitment, &alice_encrypted)
        .expect("process committed payload");

    assert_eq!(
        reveal.len(),
        32,
        "Reveal must be 32 bytes (Bob's original commitment for verification)"
    );
}

// @scenario: ble_exchange :: Commitment mismatch is rejected
#[test]
fn test_commitment_mismatch_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = alice.create_key_offer().expect("key offer");
    let (ack_bytes, bob_encrypted) = bob
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process offer");
    let (_commitment, alice_encrypted) = alice
        .process_key_ack(&ack_bytes, &bob_encrypted)
        .expect("process ack");

    // Send wrong commitment
    let bad_commitment = [0xFFu8; 32];
    let result = bob.process_committed_payload(&bad_commitment, &alice_encrypted);
    assert!(
        matches!(result, Err(ExchangeError::BleCommitmentMismatch)),
        "Mismatched commitment must be rejected with BleCommitmentMismatch"
    );
}

// ============================================================
// Phase 4: Complete Exchange
// ============================================================

// @scenario: ble_exchange :: Full 4-phase handshake happy path
#[test]
fn test_full_handshake_happy_path() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let mut alice = BleHandshakeSession::new_initiator(
        &alice_id,
        alice_card.clone(),
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card.clone(),
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    // Phase 1: Initiator creates key offer
    let offer = alice.create_key_offer().expect("key offer");

    // Phase 2: Responder processes offer, returns ack + encrypted card
    let (ack_bytes, bob_encrypted) = bob
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process offer");

    // Phase 2 (Initiator): Process ack, get commitment + encrypted card
    let (alice_commitment, alice_encrypted) = alice
        .process_key_ack(&ack_bytes, &bob_encrypted)
        .expect("process ack");

    // Phase 3: Responder processes initiator's committed payload
    let bob_reveal = bob
        .process_committed_payload(&alice_commitment, &alice_encrypted)
        .expect("process committed payload");

    // Phase 4 (Initiator): Complete with Bob's reveal
    let alice_result = alice
        .complete_exchange(&bob_reveal)
        .expect("alice complete exchange");

    // Phase 4 (Responder): Complete
    let bob_result = bob.complete_exchange(&[]).expect("bob complete exchange");

    // Verify cards were exchanged correctly
    assert_eq!(
        alice_result.remote_card.display_name, "Bob",
        "Alice must have Bob's card"
    );
    assert_eq!(
        bob_result.remote_card.display_name, "Alice",
        "Bob must have Alice's card"
    );

    // Verify identity keys match
    assert_eq!(
        alice_result.remote_card.identity_key,
        *bob_id.signing_public_key(),
        "Alice's remote card identity key must be Bob's"
    );
    assert_eq!(
        bob_result.remote_card.identity_key,
        *alice_id.signing_public_key(),
        "Bob's remote card identity key must be Alice's"
    );

    // Verify CRC16
    assert!(
        alice_result.remote_card.verify_crc16(),
        "Decrypted remote card must pass CRC16"
    );
    assert!(
        bob_result.remote_card.verify_crc16(),
        "Decrypted remote card must pass CRC16"
    );

    // Verify fields transferred
    assert_eq!(
        alice_result.remote_card.fields,
        vec![("email".to_string(), "test@example.com".to_string())],
        "Contact fields must transfer correctly"
    );

    // Both sessions must be Complete
    assert!(
        matches!(alice.state(), BleHandshakeState::Complete { .. }),
        "Alice must be in Complete state"
    );
    assert!(
        matches!(bob.state(), BleHandshakeState::Complete { .. }),
        "Bob must be in Complete state"
    );
}

// ============================================================
// Expiry & Timestamp
// ============================================================

// @scenario: ble_exchange :: Expired key offer is rejected
#[test]
fn test_expired_key_offer_rejected() {
    let bob_id = make_test_identity();
    let bob_card = make_test_card(&bob_id, "Bob");
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    // Construct a v2 offer with a timestamp far in the past
    let mut offer = vec![BLE_HANDSHAKE_VERSION];
    offer.extend_from_slice(&[1u8; 32]); // identity_pub
    offer.extend_from_slice(&[4u8; 32]); // exchange_pub (v2)
    offer.extend_from_slice(&[2u8; 32]); // ephemeral_pub
    offer.extend_from_slice(&[3u8; 16]); // nonce
    // Timestamp: 0 (epoch); observed from `now=1200` (20 min later)
    offer.extend_from_slice(&0u64.to_be_bytes());

    let result = bob.process_key_offer(&offer, 1200);
    assert!(
        matches!(result, Err(ExchangeError::BleExpired)),
        "Expired offer must be rejected with BleExpired"
    );
}

// ============================================================
// Self-Exchange Prevention
// ============================================================

// @scenario: ble_exchange :: Self-exchange rejected in handshake
#[test]
fn test_self_exchange_rejected() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");

    let mut alice_init = BleHandshakeSession::new_initiator(
        &identity,
        card.clone(),
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut alice_resp = BleHandshakeSession::new_responder(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = alice_init.create_key_offer().expect("key offer");

    // Responder has same identity key — should detect self-exchange
    let result = alice_resp.process_key_offer(
        &offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "Self-exchange must be rejected"
    );
}

// ============================================================
// Edge Cases
// ============================================================

// @scenario: ble_exchange :: Complete exchange rejected in wrong state
#[test]
fn test_complete_exchange_in_wrong_state() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let mut session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let result = session.complete_exchange(&[]);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "complete_exchange in Idle state must fail"
    );
}

// @scenario: ble_exchange :: Process committed payload rejected in wrong state
#[test]
fn test_process_committed_payload_in_wrong_state() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let mut session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let result = session.process_committed_payload(&[0u8; 32], &[0u8; 64]);
    assert!(
        matches!(result, Err(ExchangeError::InvalidState(_))),
        "process_committed_payload in Idle state must fail"
    );
}

// ============================================================
// Identity Binding (Protocol v2)
// ============================================================

// @scenario: ble_exchange :: Protocol version is 0x02
#[test]
fn test_protocol_version_is_v2() {
    assert_eq!(
        BLE_HANDSHAKE_VERSION, 0x02,
        "BLE handshake must use protocol version 2"
    );
}

// @scenario: ble_exchange :: KeyOffer includes exchange public key
#[test]
fn test_key_offer_includes_exchange_key() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");
    let mut session = BleHandshakeSession::new_initiator(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = session.create_key_offer().expect("key offer");

    // v2 KeyOffer: version(1) + identity_pub(32) + exchange_pub(32)
    //   + ephemeral_pub(32) + nonce(16) + timestamp(8) = 121 bytes
    assert_eq!(
        offer.len(),
        121,
        "v2 KeyOffer must be 121 bytes (was 89 in v1)"
    );

    // identity_pub is Ed25519 signing key
    assert_eq!(
        &offer[1..33],
        identity.signing_public_key(),
        "Bytes 1..33 must be Ed25519 signing key"
    );

    // exchange_pub is X25519 key
    assert_eq!(
        &offer[33..65],
        identity.exchange_public_key(),
        "Bytes 33..65 must be X25519 exchange key"
    );
}

// @scenario: ble_exchange :: Tampered exchange key causes decryption failure
#[test]
fn test_tampered_exchange_key_fails() {
    // If an attacker modifies the exchange_pub in a KeyOffer,
    // DH1 produces different secrets → decryption fails.
    let alice = make_test_identity();
    let bob = make_test_identity();

    let alice_card = make_test_card(&alice, "Alice");
    let bob_card = make_test_card(&bob, "Bob");

    let mut init = BleHandshakeSession::new_initiator(
        &alice,
        alice_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut resp = BleHandshakeSession::new_responder(
        &bob,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let mut offer = init.create_key_offer().unwrap();
    // Tamper with exchange_pub (bytes 33..65)
    offer[33] ^= 0xFF;

    // Responder derives a different DH1 → different session key
    let (ack, enc_bob) = resp
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .unwrap();

    // Initiator uses its real identity key for DH1 but the ack
    // was encrypted with a mismatched key → decryption fails
    let result = init.process_key_ack(&ack, &enc_bob);
    assert!(
        result.is_err(),
        "Tampered exchange key must cause handshake failure"
    );
}

// @scenario: ble_exchange :: v1 offer rejected by v2 responder
#[test]
fn test_v1_offer_rejected() {
    let bob_id = make_test_identity();
    let bob_card = make_test_card(&bob_id, "Bob");
    let mut bob = BleHandshakeSession::new_responder(
        &bob_id,
        bob_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    // Construct a v1-format offer padded to v2 length so it passes
    // the size check and reaches the version check
    let mut v1_offer = vec![0x01u8]; // v1 version
    v1_offer.extend_from_slice(&[0u8; 120]); // pad to 121 bytes

    let result = bob.process_key_offer(
        &v1_offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(
        matches!(result, Err(ExchangeError::InvalidProtocolVersion)),
        "v1 offer must be rejected by v2 responder"
    );
}

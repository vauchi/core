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

    // version(1) + identity_pub(32) + exchange_pub(32) + ephemeral_pub(32)
    //   + nonce(16) + timestamp(8) + oob_nonce(16) = 137
    assert_eq!(offer.len(), 137, "v4 KeyOffer must be exactly 137 bytes");
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

    // v3 KeyAck: version(1) + identity(32) + exchange(32) + ephemeral(32) + nonce(16)
    //            + commitment(32) + sender_timestamp(8) = 153
    assert_eq!(ack_bytes.len(), 153, "v3 KeyAck must be exactly 153 bytes");
    assert_eq!(
        ack_bytes[0], BLE_HANDSHAKE_VERSION,
        "KeyAck version must match"
    );

    // Commitment is SHA-256 of encrypted card (at offset 113..145 — v3
    // adds sender_timestamp at [145..153] after the commitment).
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
    bad_offer.extend_from_slice(&[0u8; 136]); // Pad to 137 bytes (v4 size)

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
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted_card,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
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
    let result = alice.process_key_ack(&[0u8; 113], &[0u8; 64], 0u64);
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
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
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
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process ack");

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
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
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

    assert_eq!(
        alice_result.remote_card.display_name, "Bob",
        "Alice must have Bob's card"
    );
    assert_eq!(
        bob_result.remote_card.display_name, "Alice",
        "Bob must have Alice's card"
    );

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

    assert!(
        alice_result.remote_card.verify_crc16(),
        "Decrypted remote card must pass CRC16"
    );
    assert!(
        bob_result.remote_card.verify_crc16(),
        "Decrypted remote card must pass CRC16"
    );

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

// Regression for problem record 2026-05-21-ble-aad-asymmetry.
// Pre-v3 the AAD mixed sender + receiver identities with the SENDER's
// timestamp, but the KeyAck wire format carried no timestamp slot.
// The receiver fell back to its local `self.our_timestamp` on AAD
// reconstruction, so any clock drift between the two peers (very
// common in the wild — phones with off-by-a-few-seconds clocks)
// surfaced as opaque "AEAD authentication failed" even though the
// encryption itself was correct. v3 adds `sender_timestamp` to the
// KeyAck wire and persists the initiator's offer-timestamp on the
// responder session so the reciprocal-decrypt site can match.
// @scenario: ble_exchange :: Round-trip handshake succeeds under clock drift
#[test]
fn test_full_handshake_round_trip_under_clock_drift() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    let alice_now: u64 = 1_700_000_000;
    let bob_now: u64 = alice_now + 5;

    let mut alice = BleHandshakeSession::new_initiator(&alice_id, alice_card, alice_now);
    let mut bob = BleHandshakeSession::new_responder(&bob_id, bob_card, bob_now);

    let offer = alice.create_key_offer().expect("key offer");
    let (ack_bytes, bob_encrypted) = bob
        .process_key_offer(&offer, bob_now)
        .expect("process offer");
    let (alice_commitment, alice_encrypted) = alice
        .process_key_ack(&ack_bytes, &bob_encrypted, alice_now + 1)
        .expect("process ack under drift");
    let bob_reveal = bob
        .process_committed_payload(&alice_commitment, &alice_encrypted)
        .expect("process committed payload");
    let alice_result = alice
        .complete_exchange(&bob_reveal)
        .expect("alice complete exchange under drift");
    let bob_result = bob
        .complete_exchange(&[])
        .expect("bob complete exchange under drift");

    assert_eq!(
        alice_result.remote_card.display_name, "Bob",
        "Alice must have Bob's card after handshake with 5s clock drift"
    );
    assert_eq!(
        bob_result.remote_card.display_name, "Alice",
        "Bob must have Alice's card after handshake with 5s clock drift"
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
    offer.extend_from_slice(&[0u8; 16]); // oob_nonce echo (v4, none)

    let result = bob.process_key_offer(&offer, 1200);
    assert!(
        matches!(result, Err(ExchangeError::BleExpired)),
        "Expired offer must be rejected with BleExpired"
    );
}

// ============================================================
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

// Mirrors `test_self_exchange_rejected` for the initiator's
// `process_key_ack` path. Regression for
// _private/docs/problems/2026-05-21-ble-initiator-missing-checks/.
// Without the check, an attacker who reflects the initiator's own
// KeyOffer back as a forged KeyAck reaches the commitment-derivation
// step instead of failing at the identity layer the way the
// responder does.
// @scenario: ble_exchange :: Self-exchange rejected in handshake
#[test]
fn test_initiator_process_key_ack_rejects_self_identity() {
    let identity = make_test_identity();
    let card = make_test_card(&identity, "Alice");

    // Alice (initiator) and "self-attacker" (responder) share the
    // same identity key, so the ack the attacker produces is
    // structurally identical to a legitimate ack from Alice's own
    // perspective except the `their_identity` field equals
    let mut alice = BleHandshakeSession::new_initiator(
        &identity,
        card.clone(),
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let mut attacker = BleHandshakeSession::new_responder(
        &identity,
        card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let offer = alice.create_key_offer().expect("key offer");
    // Snapshot Alice's identity key bytes from her offer — bytes 1..33
    // of the offer wire format are our_identity_key (see
    // `create_key_offer`).
    let alice_identity: [u8; 32] = offer[1..33].try_into().expect("identity slice is 32 bytes");

    // (process_key_offer on a same-identity responder would reject
    // the offer up-front per test_self_exchange_rejected), then
    // forge the their_identity field to Alice's own.
    let other_id = make_test_identity();
    let other_card = make_test_card(&other_id, "Other");
    let mut other = BleHandshakeSession::new_responder(
        &other_id,
        other_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let (mut ack_bytes, encrypted_card) = other
        .process_key_offer(
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process offer");
    ack_bytes[1..33].copy_from_slice(&alice_identity);

    let _ = attacker; // silence unused — kept for clarity above
    let result = alice.process_key_ack(
        &ack_bytes,
        &encrypted_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "Initiator must reject ack where their_identity == our_identity_key, got {:?}",
        result
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Initiator-side session timeout: if `BLE_HANDSHAKE_EXPIRY_SECS` has
// elapsed since this session sent its offer, an ack must be rejected
// regardless of its content. Defends against stuck handshakes and a
// degenerate replay where an attacker holds a captured ack for later
// delivery. Mirrors the responder-side `test_expired_key_offer_rejected`.
// _private/docs/problems/2026-05-21-ble-initiator-missing-checks/
// (Option B: deferred timestamp half).
// @scenario: ble_exchange :: Initiator rejects ack after session timeout
#[test]
fn test_initiator_process_key_ack_rejects_expired_session() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let alice_card = make_test_card(&alice_id, "Alice");
    let bob_card = make_test_card(&bob_id, "Bob");

    // Alice sends offer at t=100. The session stores 100 as `our_timestamp`.
    let mut alice = BleHandshakeSession::new_initiator(&alice_id, alice_card, 100);
    let mut bob = BleHandshakeSession::new_responder(&bob_id, bob_card, 100);

    let offer = alice.create_key_offer().expect("key offer");
    let (ack_bytes, bob_encrypted_card) =
        bob.process_key_offer(&offer, 100).expect("process offer");

    // Ack arrives at t=100 + BLE_HANDSHAKE_EXPIRY_SECS (60) + 1 = 161s
    // later — past the initiator's session-timeout horizon.
    let result = alice.process_key_ack(&ack_bytes, &bob_encrypted_card, 161);
    assert!(
        matches!(result, Err(ExchangeError::BleExpired)),
        "Initiator must reject ack arriving after session timeout, got {:?}",
        result
    );
}

// ============================================================
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
// Identity Binding (Protocol v3)
// ============================================================

// @scenario: ble_exchange :: Protocol version is 0x03
#[test]
fn test_protocol_version_is_v4() {
    assert_eq!(
        BLE_HANDSHAKE_VERSION, 0x04,
        "BLE handshake must use protocol version 4 (OOB binding slot)"
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

    // v4 KeyOffer: version(1) + identity_pub(32) + exchange_pub(32)
    //   + ephemeral_pub(32) + nonce(16) + timestamp(8) + oob_nonce(16)
    //   = 137 bytes
    assert_eq!(
        offer.len(),
        137,
        "v4 KeyOffer must be 137 bytes (121 in v2/v3, 89 in v1)"
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
    let result = init.process_key_ack(
        &ack,
        &enc_bob,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
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

    // Construct a v1-format offer padded to v4 length so it passes
    // the size check and reaches the version check
    let mut v1_offer = vec![0x01u8]; // v1 version
    v1_offer.extend_from_slice(&[0u8; 136]); // pad to 137 bytes

    let result = bob.process_key_offer(
        &v1_offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(
        matches!(result, Err(ExchangeError::InvalidProtocolVersion)),
        "v1 offer must be rejected by v2 responder"
    );
}

// ============================================================
// OOB bootstrap binding — expected-peer pin + nonce echo (v4)
// Design: _private/docs/designs/2026-06-10-oob-bootstrap-exchange-rituals-design.md
// Record: _private/docs/problems/2026-06-10-ble-unauthenticated-peer-identity
// ============================================================

fn now() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

// @scenario: ble_exchange :: Pinned initiator rejects a KeyAck from an unexpected identity
#[test]
fn test_pinned_initiator_rejects_key_ack_from_unexpected_identity() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let carol_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.expect_peer(*carol_id.signing_public_key());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());

    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_card) = bob.process_key_offer(&offer, now()).expect("process offer");

    let err = alice
        .process_key_ack(&ack, &bob_card, now())
        .expect_err("ack from bob must be rejected when carol is pinned");
    assert!(
        matches!(err, ExchangeError::IdentityMismatch),
        "expected IdentityMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: Pinned initiator accepts the expected identity
#[test]
fn test_pinned_initiator_accepts_expected_identity() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.expect_peer(*bob_id.signing_public_key());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());

    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_card) = bob.process_key_offer(&offer, now()).expect("process offer");
    let (commitment, alice_card) = alice
        .process_key_ack(&ack, &bob_card, now())
        .expect("pinned ack from the expected identity must succeed");

    assert_eq!(commitment.len(), 32, "commitment must be SHA-256 sized");
    assert!(!alice_card.is_empty(), "encrypted card must be produced");
}

// @scenario: ble_exchange :: Pinned initiator rejects a KeyAck whose exchange key differs from the OOB-pinned one
#[test]
fn test_pinned_initiator_rejects_key_ack_with_wrong_exchange_key() {
    // The critical MITM the identity pin alone does NOT stop: the Ed25519
    // identity is public, and it is never used in the DH — the X25519
    // `exchange_key` drives key agreement. A radio-range attacker presents the
    // pinned (public) identity but its OWN exchange key; without pinning the
    // exchange key too, the attacker completes the handshake as the pinned peer.
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    // Alice scanned Bob's QR: it authenticates BOTH his identity AND his
    // exchange key. She pins both.
    alice.expect_peer(*bob_id.signing_public_key());
    alice.expect_exchange_key(*bob_id.x3dh_keypair().public_key());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());

    let offer = alice.create_key_offer().expect("key offer");
    let (mut ack, bob_card) = bob.process_key_offer(&offer, now()).expect("process offer");
    // Keep Bob's identity (bytes 1..33) but substitute a foreign X25519
    // exchange key (bytes 33..65) — the DH input.
    ack[33..65].copy_from_slice(&[0x42u8; 32]);

    let err = alice
        .process_key_ack(&ack, &bob_card, now())
        .expect_err("an ack presenting the pinned identity but a foreign exchange key must abort");
    assert!(
        matches!(err, ExchangeError::ExchangeKeyMismatch),
        "expected ExchangeKeyMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: Pinned initiator accepts the expected exchange key
#[test]
fn test_pinned_initiator_accepts_expected_exchange_key() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.expect_peer(*bob_id.signing_public_key());
    alice.expect_exchange_key(*bob_id.x3dh_keypair().public_key());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());

    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_card) = bob.process_key_offer(&offer, now()).expect("process offer");
    let (commitment, alice_card) = alice
        .process_key_ack(&ack, &bob_card, now())
        .expect("an ack with the pinned exchange key must succeed");

    assert_eq!(commitment.len(), 32, "commitment must be SHA-256 sized");
    assert!(!alice_card.is_empty(), "encrypted card must be produced");
}

// @scenario: ble_exchange :: Pinned responder rejects a KeyOffer whose exchange key differs from the pinned one
#[test]
fn test_pinned_responder_rejects_offer_with_wrong_exchange_key() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.expect_peer(*alice_id.signing_public_key());
    bob.expect_exchange_key(*alice_id.x3dh_keypair().public_key());

    let mut offer = alice.create_key_offer().expect("key offer");
    // Keep Alice's identity but substitute a foreign exchange key (bytes 33..65).
    offer[33..65].copy_from_slice(&[0x37u8; 32]);

    let err = bob.process_key_offer(&offer, now()).expect_err(
        "an offer presenting the pinned identity but a foreign exchange key must abort",
    );
    assert!(
        matches!(err, ExchangeError::ExchangeKeyMismatch),
        "expected ExchangeKeyMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: Pinned responder rejects a KeyOffer from an unexpected identity
#[test]
fn test_pinned_responder_rejects_offer_from_unexpected_identity() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let carol_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.expect_peer(*carol_id.signing_public_key());

    let offer = alice.create_key_offer().expect("key offer");
    let err = bob
        .process_key_offer(&offer, now())
        .expect_err("offer from alice must be rejected when carol is pinned");
    assert!(
        matches!(err, ExchangeError::IdentityMismatch),
        "expected IdentityMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: Responder rejects a zeroed OOB echo when a nonce is required
#[test]
fn test_responder_rejects_zero_oob_nonce_when_nonzero_required() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.require_oob_nonce([7u8; 16]);

    let offer = alice.create_key_offer().expect("key offer");
    let err = bob
        .process_key_offer(&offer, now())
        .expect_err("offer without the displayed nonce echo must be rejected");
    assert!(
        matches!(err, ExchangeError::OobNonceMismatch),
        "expected OobNonceMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: OOB nonce echo roundtrip succeeds
#[test]
fn test_oob_nonce_echo_roundtrip() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let displayed_nonce = [9u8; 16];

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.set_oob_nonce(displayed_nonce);
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.require_oob_nonce(displayed_nonce);

    let offer = alice.create_key_offer().expect("key offer");
    let (ack, bob_card) = bob
        .process_key_offer(&offer, now())
        .expect("offer echoing the displayed nonce must succeed");
    let (commitment, _) = alice
        .process_key_ack(&ack, &bob_card, now())
        .expect("roundtrip completes");
    assert_eq!(commitment.len(), 32, "commitment must be SHA-256 sized");
}

// @scenario: ble_exchange :: Wrong echoed nonce is rejected (adversarial, CC-14)
#[test]
fn test_responder_rejects_wrong_oob_nonce_echo() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.set_oob_nonce([1u8; 16]);
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.require_oob_nonce([2u8; 16]);

    let offer = alice.create_key_offer().expect("key offer");
    let err = bob
        .process_key_offer(&offer, now())
        .expect_err("wrong nonce echo must be rejected");
    assert!(
        matches!(err, ExchangeError::OobNonceMismatch),
        "expected OobNonceMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: Correct nonce echo cannot smuggle a wrong identity (CC-14)
#[test]
fn test_correct_echo_with_wrong_identity_still_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();
    let carol_id = make_test_identity();
    let displayed_nonce = [5u8; 16];

    // Alice echoes the right nonce, but Bob pinned Carol — the pin
    // must win even when the echo is correct (combined constraints).
    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    alice.set_oob_nonce(displayed_nonce);
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());
    bob.require_oob_nonce(displayed_nonce);
    bob.expect_peer(*carol_id.signing_public_key());

    let offer = alice.create_key_offer().expect("key offer");
    let err = bob
        .process_key_offer(&offer, now())
        .expect_err("correct echo must not bypass the identity pin");
    assert!(
        matches!(err, ExchangeError::IdentityMismatch),
        "expected IdentityMismatch, got {err:?}"
    );
}

// @scenario: ble_exchange :: v4 KeyOffer carries the OOB nonce slot
#[test]
fn test_v4_key_offer_is_137_bytes() {
    let alice_id = make_test_identity();
    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());

    let offer = alice.create_key_offer().expect("key offer");
    assert_eq!(
        offer.len(),
        137,
        "v4 KeyOffer = v3 121 bytes + oob_nonce(16) appended"
    );
    assert_eq!(offer[0], BLE_HANDSHAKE_VERSION, "version byte leads");
}

// @scenario: ble_exchange :: Truncated v4 offer rejected (adversarial, CC-14)
#[test]
fn test_truncated_v4_offer_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice =
        BleHandshakeSession::new_initiator(&alice_id, make_test_card(&alice_id, "Alice"), now());
    let mut bob =
        BleHandshakeSession::new_responder(&bob_id, make_test_card(&bob_id, "Bob"), now());

    let offer = alice.create_key_offer().expect("key offer");
    let err = bob
        .process_key_offer(&offer[..offer.len() - 1], now())
        .expect_err("truncated offer must be rejected");
    assert!(
        matches!(err, ExchangeError::InvalidBleFormat),
        "expected InvalidBleFormat, got {err:?}"
    );
}

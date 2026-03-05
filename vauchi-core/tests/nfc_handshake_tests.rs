// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::{
    ExchangeError, ExchangeNfc, NfcHandshakeSession, NfcHandshakeState, X3DHKeyPair,
};
use vauchi_core::identity::Identity;

fn make_test_identity() -> Identity {
    Identity::create("Test")
}

// ============================================================
// Phase 1: Key Offer
// ============================================================

#[test]
fn test_initiator_creates_key_offer() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Alice".to_string());

    assert!(matches!(session.state(), NfcHandshakeState::Idle));

    let offer_bytes = session
        .create_key_offer(&identity)
        .expect("key offer should succeed");

    assert!(matches!(
        session.state(),
        NfcHandshakeState::KeyOfferSent { .. }
    ));
    assert_eq!(offer_bytes.len(), 174, "Key offer must be ExchangeNfc size");

    let parsed = ExchangeNfc::from_bytes(&offer_bytes).expect("should parse as ExchangeNfc");
    assert!(parsed.verify_signature());
    assert!(!parsed.is_expired());
}

#[test]
fn test_double_key_offer_rejected() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    session.create_key_offer(&identity).expect("first offer");
    let result = session.create_key_offer(&identity);
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// ============================================================
// Full Handshake
// ============================================================

#[test]
fn test_full_handshake_happy_path() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    // Phase 1: Alice creates key offer
    let offer = alice.create_key_offer(&alice_id).expect("key offer");

    // Phase 2: Bob processes offer, returns ack + encrypted card
    let (ack_bytes, bob_encrypted_card) = bob.process_key_offer(&bob_id, &offer).expect("key ack");

    // Phase 2 (Alice): Alice processes ack, decrypts Bob's card
    let alice_encrypted_card = alice
        .process_key_ack(&ack_bytes, &bob_encrypted_card)
        .expect("process ack");

    // Phase 3: Bob decrypts Alice's card
    let bob_result = bob
        .process_encrypted_card(&alice_encrypted_card)
        .expect("process card");

    // Alice confirms send success
    let alice_result = alice.confirm_send_success().expect("confirm send");

    // Verify both sides have each other's data
    assert_eq!(alice_result.remote_card.display_name, "Bob");
    assert_eq!(bob_result.remote_card.display_name, "Alice");
    assert_eq!(
        alice_result.remote_card.identity_key,
        *bob_id.signing_public_key()
    );
    assert_eq!(
        bob_result.remote_card.identity_key,
        *alice_id.signing_public_key()
    );

    // Verify CRC16 on both sides
    assert!(alice_result.remote_card.verify_crc16());
    assert!(bob_result.remote_card.verify_crc16());

    // Verify states
    assert!(matches!(alice.state(), NfcHandshakeState::Complete { .. }));
    assert!(matches!(bob.state(), NfcHandshakeState::Complete { .. }));
}

// ============================================================
// Failure Cases
// ============================================================

#[test]
fn test_expired_key_offer_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let x3dh = X3DHKeyPair::generate();
    let token = [0xAA; 32];
    // Timestamp 0 = far in the past (expired)
    let expired_nfc = ExchangeNfc::generate_with_timestamp(&alice_id, &x3dh, token, 0);
    let expired_bytes = expired_nfc.to_bytes();

    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());
    let result = bob.process_key_offer(&bob_id, &expired_bytes);
    assert!(matches!(result, Err(ExchangeError::NfcExpired)));
}

#[test]
fn test_tampered_ciphertext_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice.create_key_offer(&alice_id).expect("key offer");
    let (ack_bytes, mut bob_encrypted_card) =
        bob.process_key_offer(&bob_id, &offer).expect("key ack");

    // Tamper with encrypted card
    if let Some(byte) = bob_encrypted_card.last_mut() {
        *byte ^= 0xFF;
    }

    let result = alice.process_key_ack(&ack_bytes, &bob_encrypted_card);
    assert!(matches!(result, Err(ExchangeError::NfcDecryptionFailed)));
}

#[test]
fn test_tampered_phase3_ciphertext_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice.create_key_offer(&alice_id).expect("key offer");
    let (ack_bytes, bob_encrypted_card) = bob.process_key_offer(&bob_id, &offer).expect("key ack");
    let mut alice_encrypted_card = alice
        .process_key_ack(&ack_bytes, &bob_encrypted_card)
        .expect("process ack");

    // Tamper with Alice's encrypted card
    if let Some(byte) = alice_encrypted_card.last_mut() {
        *byte ^= 0xFF;
    }

    let result = bob.process_encrypted_card(&alice_encrypted_card);
    assert!(matches!(result, Err(ExchangeError::NfcDecryptionFailed)));
}

#[test]
fn test_invalid_state_transitions() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    // Cannot process key ack from Idle state
    let result = session.process_key_ack(&[0; 174], &[0; 100]);
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));

    // Cannot process encrypted card from Idle state
    let result = session.process_encrypted_card(&[0; 100]);
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));

    // Cannot confirm send from Idle state
    let result = session.confirm_send_success();
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// ============================================================
// Relay Fallback
// ============================================================

#[test]
fn test_relay_fallback_from_key_ack_received() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice.create_key_offer(&alice_id).expect("key offer");
    let (_ack_bytes, _bob_encrypted_card) =
        bob.process_key_offer(&bob_id, &offer).expect("key ack");

    // Bob's tap drops after key exchange — fallback to relay
    let result = bob.enter_relay_fallback();
    assert!(result.is_ok());
    assert!(matches!(
        bob.state(),
        NfcHandshakeState::RelayFallback { .. }
    ));
}

#[test]
fn test_relay_fallback_from_idle_fails() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    let result = session.enter_relay_fallback();
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

#[test]
fn test_relay_fallback_without_shared_key_fails() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    // Create key offer (moves to KeyOfferSent but no shared key yet)
    session.create_key_offer(&identity).expect("key offer");

    let result = session.enter_relay_fallback();
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// ============================================================
// Identity Key Verification
// ============================================================

#[test]
fn test_exchange_preserves_identity_keys() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice.create_key_offer(&alice_id).expect("key offer");
    let (ack_bytes, bob_encrypted_card) = bob.process_key_offer(&bob_id, &offer).expect("key ack");
    let alice_encrypted_card = alice
        .process_key_ack(&ack_bytes, &bob_encrypted_card)
        .expect("process ack");
    let bob_result = bob
        .process_encrypted_card(&alice_encrypted_card)
        .expect("process card");
    let alice_result = alice.confirm_send_success().expect("confirm send");

    // Both cards contain the correct signing public keys
    assert_eq!(
        alice_result.local_card.identity_key,
        *alice_id.signing_public_key()
    );
    assert_eq!(
        bob_result.local_card.identity_key,
        *bob_id.signing_public_key()
    );
}

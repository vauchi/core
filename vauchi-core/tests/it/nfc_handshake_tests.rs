// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::exchange::{
    ExchangeError, ExchangeNfc, NfcCardPayload, NfcHandshakeSession, NfcHandshakeState, X3DHKeyPair,
};
use vauchi_core::identity::Identity;

fn make_test_identity() -> Identity {
    Identity::create("Test", 0)
}

// ============================================================
// Phase 1: Key Offer
// ============================================================

// @internal
#[test]
fn test_initiator_creates_key_offer() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Alice".to_string());

    assert!(matches!(session.state(), NfcHandshakeState::Idle));

    let offer_bytes = session
        .create_key_offer(
            &identity,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer should succeed");

    assert!(matches!(
        session.state(),
        NfcHandshakeState::KeyOfferSent { .. }
    ));
    assert_eq!(offer_bytes.len(), 174, "Key offer must be ExchangeNfc size");

    let parsed = ExchangeNfc::from_bytes(&offer_bytes).expect("should parse as ExchangeNfc");
    assert!(parsed.verify_signature());
    assert!(!parsed.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()));
}

// @internal
#[test]
fn test_double_key_offer_rejected() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    session
        .create_key_offer(
            &identity,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("first offer");
    let result = session.create_key_offer(
        &identity,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// ============================================================
// ============================================================

// @internal
#[test]
fn test_full_handshake_happy_path() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    // Phase 1: Alice creates key offer
    let offer = alice
        .create_key_offer(
            &alice_id,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");

    // Phase 2: Bob processes offer, returns ack + encrypted card
    let (ack_bytes, bob_encrypted_card) = bob
        .process_key_offer(
            &bob_id,
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key ack");

    // Phase 2 (Alice): Alice processes ack, decrypts Bob's card
    let alice_encrypted_card = alice
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted_card,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process ack");

    // Phase 3: Bob decrypts Alice's card
    let bob_result = bob
        .process_encrypted_card(&alice_encrypted_card)
        .expect("process card");

    let alice_result = alice.confirm_send_success().expect("confirm send");

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

    assert!(alice_result.remote_card.verify_crc16());
    assert!(bob_result.remote_card.verify_crc16());

    assert!(matches!(alice.state(), NfcHandshakeState::Complete { .. }));
    assert!(matches!(bob.state(), NfcHandshakeState::Complete { .. }));
}

// ============================================================
// ============================================================

// @internal
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
    let result = bob.process_key_offer(
        &bob_id,
        &expired_bytes,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(matches!(result, Err(ExchangeError::NfcExpired)));
}

// Mirror of BLE's `test_self_exchange_rejected` for the NFC
// `process_key_offer` path. Without this check, a reflection attack
// (an attacker that replays the responder's own offer back as if from
// the initiator) reaches key derivation and only fails later at AEAD
// decryption — instead of being rejected at the identity layer the way
// BLE already does (`ble_handshake.rs:340`). Regression for
// `_private/docs/problems/2026-05-21-nfc-handshake-asymmetric-defenses/`
// (closes F-HIGH-3; F-HIGH-2 KDF identity-binding still open).
// @scenario: nfc_exchange :: Self-exchange rejected in handshake
#[test]
fn test_nfc_process_key_offer_rejects_self_identity() {
    let identity = make_test_identity();

    // Initiator and responder share the same identity — simulating a
    // reflection attack on the responder side.
    let mut alice_init = NfcHandshakeSession::new_initiator(&identity, "Alice".to_string());
    let mut alice_resp = NfcHandshakeSession::new_responder(&identity, "Alice".to_string());

    let offer = alice_init
        .create_key_offer(
            &identity,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");

    let result = alice_resp.process_key_offer(
        &identity,
        &offer,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "NFC responder must reject offer with their_identity == our_identity_key, got {:?}",
        result
    );
}

// @internal
#[test]
fn test_tampered_ciphertext_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice
        .create_key_offer(
            &alice_id,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");
    let (ack_bytes, mut bob_encrypted_card) = bob
        .process_key_offer(
            &bob_id,
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key ack");

    if let Some(byte) = bob_encrypted_card.last_mut() {
        *byte ^= 0xFF;
    }

    let result = alice.process_key_ack(
        &ack_bytes,
        &bob_encrypted_card,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    assert!(matches!(result, Err(ExchangeError::NfcDecryptionFailed)));
}

// @internal
#[test]
fn test_tampered_phase3_ciphertext_rejected() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice
        .create_key_offer(
            &alice_id,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");
    let (ack_bytes, bob_encrypted_card) = bob
        .process_key_offer(
            &bob_id,
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key ack");
    let mut alice_encrypted_card = alice
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted_card,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process ack");

    // Tamper with Alice's encrypted card
    if let Some(byte) = alice_encrypted_card.last_mut() {
        *byte ^= 0xFF;
    }

    let result = bob.process_encrypted_card(&alice_encrypted_card);
    assert!(matches!(result, Err(ExchangeError::NfcDecryptionFailed)));
}

// @internal
#[test]
fn test_invalid_state_transitions() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    // Cannot process key ack from Idle state
    let result = session.process_key_ack(&[0; 174], &[0; 100], 0u64);
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

// @internal
#[test]
fn test_relay_fallback_from_key_ack_received() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice
        .create_key_offer(
            &alice_id,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");
    let (_ack_bytes, _bob_encrypted_card) = bob
        .process_key_offer(
            &bob_id,
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key ack");

    // Bob's tap drops after key exchange — fallback to relay
    let result = bob.enter_relay_fallback();
    result.expect("expected success");
    assert!(matches!(
        bob.state(),
        NfcHandshakeState::RelayFallback { .. }
    ));
}

// @internal
#[test]
fn test_relay_fallback_from_idle_fails() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    let result = session.enter_relay_fallback();
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// @internal
#[test]
fn test_relay_fallback_without_shared_key_fails() {
    let identity = make_test_identity();
    let mut session = NfcHandshakeSession::new_initiator(&identity, "Test".to_string());

    // Create key offer (moves to KeyOfferSent but no shared key yet)
    session
        .create_key_offer(
            &identity,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");

    let result = session.enter_relay_fallback();
    assert!(matches!(result, Err(ExchangeError::InvalidState(_))));
}

// ============================================================
// ============================================================

// @internal
#[test]
fn test_exchange_preserves_identity_keys() {
    let alice_id = make_test_identity();
    let bob_id = make_test_identity();

    let mut alice = NfcHandshakeSession::new_initiator(&alice_id, "Alice".to_string());
    let mut bob = NfcHandshakeSession::new_responder(&bob_id, "Bob".to_string());

    let offer = alice
        .create_key_offer(
            &alice_id,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key offer");
    let (ack_bytes, bob_encrypted_card) = bob
        .process_key_offer(
            &bob_id,
            &offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("key ack");
    let alice_encrypted_card = alice
        .process_key_ack(
            &ack_bytes,
            &bob_encrypted_card,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .expect("process ack");
    let bob_result = bob
        .process_encrypted_card(&alice_encrypted_card)
        .expect("process card");
    let alice_result = alice.confirm_send_success().expect("confirm send");

    assert_eq!(
        alice_result.local_card.identity_key,
        *alice_id.signing_public_key()
    );
    assert_eq!(
        bob_result.local_card.identity_key,
        *bob_id.signing_public_key()
    );
}

// ============================================================
// ============================================================

proptest! {
// @internal
    #[test]
    fn prop_handshake_succeeds_with_any_display_names(
        alice_name in "[a-zA-Z0-9 ]{1,100}",
        bob_name in "[a-zA-Z0-9 ]{1,100}",
    ) {
        let alice_id = make_test_identity();
        let bob_id = make_test_identity();

        let mut alice = NfcHandshakeSession::new_initiator(&alice_id, alice_name.clone());
        let mut bob = NfcHandshakeSession::new_responder(&bob_id, bob_name.clone());

        let offer = alice.create_key_offer(&alice_id, vauchi_core::clock::SystemClock::shared().unix_seconds()).unwrap();
        let (ack, bob_card) = bob.process_key_offer(&bob_id, &offer, vauchi_core::clock::SystemClock::shared().unix_seconds()).unwrap();
        let alice_card = alice.process_key_ack(&ack, &bob_card, vauchi_core::clock::SystemClock::shared().unix_seconds()).unwrap();
        let bob_result = bob.process_encrypted_card(&alice_card).unwrap();
        let alice_result = alice.confirm_send_success().unwrap();

        prop_assert_eq!(&alice_result.remote_card.display_name, &bob_name);
        prop_assert_eq!(&bob_result.remote_card.display_name, &alice_name);
        prop_assert!(alice_result.remote_card.verify_crc16());
        prop_assert!(bob_result.remote_card.verify_crc16());
    }

// @internal
    #[test]
    fn prop_card_payload_roundtrip(
        name in "[\\w]{0,200}",
        key_byte in 0u8..=255u8,
        ex_byte in 0u8..=255u8,
    ) {
        let payload = NfcCardPayload::new(
            [key_byte; 32],
            name.clone(),
            [ex_byte; 32],
        );
        let bytes = payload.to_bytes().unwrap();
        let restored = NfcCardPayload::from_bytes(&bytes).unwrap();
        prop_assert_eq!(restored.identity_key, [key_byte; 32]);
        prop_assert_eq!(&restored.display_name, &name);
        prop_assert_eq!(restored.exchange_key, [ex_byte; 32]);
        prop_assert!(restored.verify_crc16());
    }
}

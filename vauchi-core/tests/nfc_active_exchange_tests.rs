// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for NFC Active Exchange (Feature B)
//!
//! Feature file: features/contact_exchange.feature @nfc @active
//!
//! Phone-to-phone NFC tap exchange. A single tap replaces both QR scan
//! and proximity verification. APDU protocol over HCE.

use vauchi_core::exchange::{
    ExchangeError, ExchangeEvent, ExchangeNfc, ExchangeSession, ExchangeState, ExchangeTransport,
    MockProximityVerifier, X3DHKeyPair, NFC_PAYLOAD_SIZE,
};
use vauchi_core::{ContactCard, Identity};

// ============================================================
// Error variants
// ============================================================

#[test]
fn test_nfc_error_variants_exist() {
    // Verify the NFC error variants are usable
    let err1 = ExchangeError::InvalidNfcFormat;
    let err2 = ExchangeError::NfcExpired;
    let err3 = ExchangeError::NfcSessionLost;
    let err4 = ExchangeError::NfcNotSupported;

    assert_eq!(format!("{}", err1), "Invalid NFC payload format");
    assert_eq!(format!("{}", err2), "NFC payload has expired");
    assert_eq!(format!("{}", err3), "NFC session lost during exchange");
    assert_eq!(format!("{}", err4), "NFC not supported on this device");
}

// ============================================================
// ExchangeNfc payload
// ============================================================

#[test]
fn test_nfc_generate() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);

    assert_eq!(payload.identity_key(), identity.signing_public_key());
    assert_eq!(payload.exchange_key(), ephemeral.public_key());
    assert!(!payload.is_expired());
    assert!(payload.verify_signature());
}

#[test]
fn test_nfc_roundtrip() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let bytes = payload.to_bytes();

    assert_eq!(bytes.len(), NFC_PAYLOAD_SIZE);
    assert_eq!(&bytes[0..4], b"VNFC");

    let parsed = ExchangeNfc::from_bytes(&bytes).expect("Should parse valid payload");

    assert_eq!(parsed.identity_key(), payload.identity_key());
    assert_eq!(parsed.exchange_key(), payload.exchange_key());
    assert_eq!(parsed.token(), payload.token());
    assert_eq!(parsed.timestamp(), payload.timestamp());
    assert!(parsed.verify_signature());
}

#[test]
fn test_nfc_signature_verify() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);

    assert!(
        payload.verify_signature(),
        "Fresh payload should have valid signature"
    );
}

#[test]
fn test_nfc_tamper_rejection() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let mut bytes = payload.to_bytes();

    // Tamper with exchange key
    bytes[38] ^= 0xFF;

    let parsed = ExchangeNfc::from_bytes(&bytes).expect("Should parse bytes");
    assert!(
        !parsed.verify_signature(),
        "Tampered payload should fail signature check"
    );
}

#[test]
fn test_nfc_magic_check() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let mut bytes = payload.to_bytes();

    // Wrong magic
    bytes[0..4].copy_from_slice(b"XXXX");

    let result = ExchangeNfc::from_bytes(&bytes);
    assert!(
        matches!(result, Err(ExchangeError::InvalidNfcFormat)),
        "Wrong magic should be rejected"
    );
}

#[test]
fn test_nfc_version_check() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let mut bytes = payload.to_bytes();

    // Wrong version
    bytes[4] = 99;

    let result = ExchangeNfc::from_bytes(&bytes);
    assert!(
        matches!(result, Err(ExchangeError::InvalidProtocolVersion)),
        "Wrong version should be rejected"
    );
}

#[test]
fn test_nfc_too_short_payload() {
    let result = ExchangeNfc::from_bytes(&[0u8; 50]);
    assert!(
        matches!(result, Err(ExchangeError::InvalidNfcFormat)),
        "Short payload should be rejected"
    );
}

#[test]
fn test_nfc_expiry() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 2 minutes ago — should be expired (NFC is 60s)
    let old_payload =
        ExchangeNfc::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now - 120);
    assert!(
        old_payload.is_expired(),
        "2-minute old NFC payload should be expired"
    );

    // Just now — should not be expired
    let fresh_payload = ExchangeNfc::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now);
    assert!(
        !fresh_payload.is_expired(),
        "Fresh payload should not be expired"
    );
}

#[test]
fn test_nfc_clock_drift_tolerance() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 30 seconds ago — should still be valid (within 60s expiry)
    let payload = ExchangeNfc::generate_with_timestamp(&identity, &ephemeral, [0u8; 32], now - 30);
    assert!(
        !payload.is_expired(),
        "30s-old NFC payload should still be valid"
    );
}

#[test]
fn test_nfc_different_ephemerals() {
    let identity = Identity::create("Alice");
    let eph1 = X3DHKeyPair::generate();
    let eph2 = X3DHKeyPair::generate();

    let p1 = ExchangeNfc::generate(&identity, &eph1);
    let p2 = ExchangeNfc::generate(&identity, &eph2);

    assert_eq!(p1.identity_key(), p2.identity_key(), "Same identity");
    assert_ne!(p1.exchange_key(), p2.exchange_key(), "Different ephemerals");
    assert_ne!(p1.token(), p2.token(), "Different tokens");
}

#[test]
fn test_nfc_payload_size() {
    assert_eq!(NFC_PAYLOAD_SIZE, 174, "NFC payload should be 174 bytes");

    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let bytes = payload.to_bytes();

    assert_eq!(bytes.len(), 174);
}

#[test]
fn test_nfc_identity_key_matches_signer() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let payload = ExchangeNfc::generate(&identity, &ephemeral);

    // The identity key in the payload should be the signing key
    assert_eq!(payload.identity_key(), identity.signing_public_key());

    // The exchange key should NOT be the identity key
    assert_ne!(payload.exchange_key(), payload.identity_key());
}

// ============================================================
// APDU protocol
// ============================================================

// We need access to the apdu module — it's re-exported via nfc_active
// but since the module is private by default, we test through the testing feature
// or via the public exports. For now, test through ExchangeNfc bytes.

#[test]
fn test_nfc_apdu_select_build() {
    // Test that we can build a SELECT command — this tests the APDU protocol
    // indirectly since the module is private when not using testing feature.
    // The key assertion is that ExchangeNfc payloads can be embedded in APDU.
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();
    let payload = ExchangeNfc::generate(&identity, &ephemeral);
    let bytes = payload.to_bytes();

    // Payload should be embeddable in a single APDU (< 255 bytes)
    assert!(bytes.len() < 255, "NFC payload should fit in a single APDU");
}

// ============================================================
// Session integration
// ============================================================

#[test]
fn test_nfc_session_starts_awaiting_tap() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let session = ExchangeSession::new_nfc(identity, card, proximity);

    assert!(
        matches!(session.state(), ExchangeState::AwaitingNfcTap),
        "NFC session should start in AwaitingNfcTap"
    );
    assert_eq!(session.transport(), ExchangeTransport::Nfc);
}

#[test]
fn test_nfc_tap_transitions_to_key_agreement() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_nfc(alice_identity, alice_card, proximity);

    // Bob generates his NFC payload
    let bob_eph = X3DHKeyPair::generate();
    let bob_payload = ExchangeNfc::generate(&bob_identity, &bob_eph);

    // Alice receives Bob's tap
    alice_session
        .apply(ExchangeEvent::NfcTapComplete {
            their_payload: bob_payload.to_bytes().to_vec(),
        })
        .expect("Should transition to AwaitingKeyAgreement");

    assert!(
        matches!(
            alice_session.state(),
            ExchangeState::AwaitingKeyAgreement { .. }
        ),
        "Should be in AwaitingKeyAgreement after tap"
    );
}

#[test]
fn test_nfc_invalid_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_nfc(identity, card, proximity);

    // Try with garbage payload — should fail (invalid NFC format)
    let result = session.apply(ExchangeEvent::NfcTapComplete {
        their_payload: vec![0u8; 174],
    });
    assert!(
        matches!(result, Err(ExchangeError::InvalidNfcFormat)),
        "Garbage NFC payload should be rejected"
    );
}

#[test]
fn test_nfc_expired_payload_rejected_by_session() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut alice_session = ExchangeSession::new_nfc(alice_identity, alice_card, proximity);

    // Bob's expired payload
    let bob_eph = X3DHKeyPair::generate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired =
        ExchangeNfc::generate_with_timestamp(&bob_identity, &bob_eph, [0u8; 32], now - 120);

    let result = alice_session.apply(ExchangeEvent::NfcTapComplete {
        their_payload: expired.to_bytes().to_vec(),
    });
    assert!(
        matches!(result, Err(ExchangeError::NfcExpired)),
        "Expired NFC payload should be rejected by session"
    );
}

#[test]
fn test_nfc_rejects_wrong_transport() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    // QR session
    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    let result = session.apply(ExchangeEvent::NfcTapComplete {
        their_payload: vec![],
    });
    assert!(
        result.is_err(),
        "NFC events should be rejected on non-NFC transport"
    );
}

#[test]
fn test_nfc_self_exchange_rejected() {
    let alice_identity = Identity::create("Alice");

    // Generate NFC payload with Alice's own identity
    let eph = X3DHKeyPair::generate();
    let self_payload = ExchangeNfc::generate(&alice_identity, &eph);
    let self_bytes = self_payload.to_bytes().to_vec();

    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();

    let mut session = ExchangeSession::new_nfc(alice_identity, card, proximity);

    let result = session.apply(ExchangeEvent::NfcTapComplete {
        their_payload: self_bytes,
    });
    assert!(
        matches!(result, Err(ExchangeError::SelfExchange)),
        "NFC self-exchange should be rejected"
    );
}

// ============================================================
// Full NFC lifecycle
// ============================================================

#[test]
fn test_nfc_full_exchange_via_session() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // Both create NFC sessions (start in AwaitingNfcTap)
    let mut alice_session = ExchangeSession::new_nfc(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
    );
    let mut bob_session = ExchangeSession::new_nfc(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
    );

    // In real flow, each side generates an ExchangeNfc from its session's
    // identity + ephemeral. Since identity is moved into the session and not
    // accessible, we build payloads with separate identities for the test.
    let alice_id2 = Identity::create("Alice2");
    let bob_id2 = Identity::create("Bob2");

    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_nfc = ExchangeNfc::generate(&alice_id2, &alice_eph);
    let bob_nfc = ExchangeNfc::generate(&bob_id2, &bob_eph);

    // Step 1: NFC tap — both receive each other's payload
    alice_session
        .apply(ExchangeEvent::NfcTapComplete {
            their_payload: bob_nfc.to_bytes().to_vec(),
        })
        .unwrap();
    bob_session
        .apply(ExchangeEvent::NfcTapComplete {
            their_payload: alice_nfc.to_bytes().to_vec(),
        })
        .unwrap();

    // Both should be in AwaitingKeyAgreement
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));

    // Step 2: Key agreement (symmetric DH)
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Step 3: Exchange cards
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    assert!(matches!(
        alice_session.state(),
        ExchangeState::Complete { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::Complete { .. }
    ));
}

#[test]
fn test_nfc_full_exchange_payload_crypto() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let alice_id = Identity::create("Alice");
    let bob_id = Identity::create("Bob");

    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    let alice_nfc = ExchangeNfc::generate(&alice_id, &alice_eph);
    let bob_nfc = ExchangeNfc::generate(&bob_id, &bob_eph);

    // Both parse each other's payload
    let alice_parsed = ExchangeNfc::from_bytes(&bob_nfc.to_bytes()).unwrap();
    let bob_parsed = ExchangeNfc::from_bytes(&alice_nfc.to_bytes()).unwrap();

    assert!(alice_parsed.verify_signature());
    assert!(bob_parsed.verify_signature());

    // Symmetric DH
    let alice_shared = alice_eph.diffie_hellman(alice_parsed.exchange_key());
    let bob_shared = bob_eph.diffie_hellman(bob_parsed.exchange_key());

    assert_eq!(alice_shared, bob_shared, "NFC symmetric DH should match");

    let key = vauchi_core::crypto::SymmetricKey::from_bytes(alice_shared);
    let key2 = vauchi_core::crypto::SymmetricKey::from_bytes(bob_shared);

    let msg = b"Hello via NFC!";
    let ct = encrypt(&key, msg).unwrap();
    let pt = decrypt(&key2, &ct).unwrap();
    assert_eq!(pt, msg);
}

#[test]
fn test_nfc_key_independence_from_qr() {
    // NFC and QR use independent ephemeral keys — shared secrets should differ

    // NFC path: fresh ephemerals
    let alice_nfc_eph = X3DHKeyPair::generate();
    let bob_nfc_eph = X3DHKeyPair::generate();
    let nfc_shared = alice_nfc_eph.diffie_hellman(bob_nfc_eph.public_key());

    // QR path: also uses fresh ephemerals (mutual QR)
    let alice_qr_eph = X3DHKeyPair::generate();
    let bob_qr_eph = X3DHKeyPair::generate();
    let qr_shared = alice_qr_eph.diffie_hellman(bob_qr_eph.public_key());

    assert_ne!(
        nfc_shared, qr_shared,
        "NFC and QR should produce different shared secrets"
    );
}

#[test]
fn test_nfc_apdu_round_trip_simulation() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_eph = X3DHKeyPair::generate();
    let bob_eph = X3DHKeyPair::generate();

    // Alice generates her NFC payload
    let alice_payload = ExchangeNfc::generate(&alice_identity, &alice_eph);
    let alice_bytes = alice_payload.to_bytes();

    // Bob generates his NFC payload
    let bob_payload = ExchangeNfc::generate(&bob_identity, &bob_eph);
    let bob_bytes = bob_payload.to_bytes();

    // Simulate APDU exchange: Alice sends, Bob parses, Bob sends, Alice parses
    let bob_received = ExchangeNfc::from_bytes(&alice_bytes).unwrap();
    let alice_received = ExchangeNfc::from_bytes(&bob_bytes).unwrap();

    // Both verify
    assert!(bob_received.verify_signature());
    assert!(alice_received.verify_signature());

    // Both compute shared secret
    let alice_secret = alice_eph.diffie_hellman(alice_received.exchange_key());
    let bob_secret = bob_eph.diffie_hellman(bob_received.exchange_key());

    assert_eq!(alice_secret, bob_secret);
}

#[test]
fn test_nfc_expired_payload_rejection() {
    let identity = Identity::create("Alice");
    let ephemeral = X3DHKeyPair::generate();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let expired = ExchangeNfc::generate_with_timestamp(
        &identity,
        &ephemeral,
        [0u8; 32],
        now - 120, // 2 minutes ago
    );

    assert!(expired.is_expired());

    // Verify it round-trips (parsing doesn't check expiry)
    let bytes = expired.to_bytes();
    let parsed = ExchangeNfc::from_bytes(&bytes).unwrap();
    assert!(parsed.is_expired());
}

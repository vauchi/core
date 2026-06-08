// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Crypto Ceremony Tests (Item 198)
//!
//! Tests the full X3DH key agreement ceremony at the application layer,
//! not just serialization or state machine transitions. Each test exercises
//! real crypto operations and verifies shared secret properties.
//!
//! Feature file: features/contact_exchange.feature @security @exchange

use vauchi_core::crypto::{decrypt, encrypt};
use vauchi_core::exchange::*;
use vauchi_core::{ContactCard, Identity};

// ============================================================
// ============================================================

/// Verify QR session key agreement produces matching keys and they
/// are usable for bidirectional encryption. This is the core ceremony test.
// @scenario: contact_exchange :: X3DH key agreement during exchange
// @scenario: contact_exchange :: Exchange creates mutual keys
// @internal
#[test]
fn test_qr_ceremony_shared_keys_match_and_encrypt() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();

    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let alice_key = match alice_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().clone(),
        _ => panic!("Alice should be in Complete state"),
    };
    let bob_key = match bob_session.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().clone(),
        _ => panic!("Bob should be in Complete state"),
    };

    // Keys must match
    assert_eq!(
        alice_key.as_bytes(),
        bob_key.as_bytes(),
        "Session key agreement must produce matching keys"
    );

    // Keys must not be degenerate
    assert_ne!(
        alice_key.as_bytes(),
        &[0u8; 32],
        "Shared key must not be all zeros"
    );

    let msg = b"Ceremony test message";
    let ct = encrypt(&alice_key, msg).unwrap();
    let pt = decrypt(&bob_key, &ct).unwrap();
    assert_eq!(pt, msg, "Bob should decrypt Alice's message");
}

// ============================================================
// Tampered wire format: ephemeral and sender keys
// ============================================================

/// Tampered ephemeral key in wire format causes AEAD decryption failure.
///
/// If an attacker modifies the ephemeral_public_key in transit, the
/// recipient derives a different shared secret, and AEAD tag verification
/// fails. This is a ceremony-level integrity test.
// @scenario: security :: Man-in-the-middle detection during exchange
// @internal
#[test]
fn test_tampered_ephemeral_key_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let alice_identity = [0x41u8; 32];

    let (mut msg, _alice_secret) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &alice_identity, "Alice")
            .unwrap();

    // Tamper with the ephemeral public key (change one byte). The
    // newtype guards swap-arg bugs, not byte mutation — tampering
    // tests unwrap/rewrap to flip a byte.
    let mut tampered = msg.ephemeral_public_key.into_bytes();
    tampered[0] ^= 0x01;
    msg.ephemeral_public_key = vauchi_core::identifiers::DhPublicKey::from_bytes(tampered);

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when ephemeral key is tampered"
    );
}

/// Tampered sender_exchange_key causes DH1 identity binding mismatch.
// @scenario: security :: Man-in-the-middle detection during exchange
// @internal
#[test]
fn test_tampered_sender_exchange_key_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (mut msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x01u8; 32], "Alice").unwrap();

    // Tamper with the identity binding component (see comment on
    // the ephemeral-key tampering test above for the unwrap/rewrap
    // rationale).
    let mut tampered = msg.sender_exchange_key.into_bytes();
    tampered[0] ^= 0x01;
    msg.sender_exchange_key = vauchi_core::identifiers::DhPublicKey::from_bytes(tampered);

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when sender exchange key is tampered"
    );
}

/// Tampered ciphertext causes AEAD tag failure.
// @scenario: security :: Man-in-the-middle detection during exchange
// @internal
#[test]
fn test_tampered_ciphertext_causes_decrypt_failure() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (mut msg, _) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x01u8; 32], "Alice").unwrap();

    // Tamper with the ciphertext (flip a byte)
    if let Some(byte) = msg.ciphertext.last_mut() {
        *byte ^= 0x01;
    }

    let result = msg.decrypt(&bob);

    assert!(
        result.is_err(),
        "Decryption must fail when ciphertext is tampered"
    );
}

// ============================================================
// Each DH component contributes to the shared secret
// ============================================================

/// Different identity keys (DH1) produce different shared secrets.
// @scenario: contact_exchange :: Exchange verifies identity
// @scenario: contact_exchange :: Identity mismatch detection
// @internal
#[test]
fn test_identity_binding_changes_shared_secret() {
    let alice1 = X3DHKeyPair::generate();
    let alice2 = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (secret1, _) = X3DH::initiate(&alice1, bob.public_key()).unwrap();
    let (secret2, _) = X3DH::initiate(&alice2, bob.public_key()).unwrap();

    assert_ne!(
        secret1.as_bytes(),
        secret2.as_bytes(),
        "Different identity keys must produce different secrets (DH1 contribution)"
    );
}

/// Same identity keys with different ephemerals produce different secrets.
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
// @internal
#[test]
fn test_ephemeral_changes_shared_secret() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (secret1, eph1) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    let (secret2, eph2) = X3DH::initiate(&alice, bob.public_key()).unwrap();

    assert_ne!(eph1, eph2, "Each initiation must use a fresh ephemeral");
    assert_ne!(
        secret1.as_bytes(),
        secret2.as_bytes(),
        "Different ephemerals must produce different secrets (DH2 contribution)"
    );
}

// ============================================================
// Transport isolation: NFC and QR sessions produce independent secrets
// ============================================================

/// Independent sessions using fresh ephemerals produce different shared secrets.
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
// @internal
#[test]
fn test_independent_sessions_produce_different_secrets() {
    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // --- QR session 1 ---
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let qr_alice_proximity = MockProximityVerifier::success();
    let mut qr_alice = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        qr_alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    let qr_bob_proximity = MockProximityVerifier::success();
    let mut qr_bob = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        qr_bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    qr_alice.apply(ExchangeEvent::StartQR).unwrap();
    qr_bob.apply(ExchangeEvent::StartQR).unwrap();
    let qr_a = qr_alice.qr().unwrap().clone();
    let qr_b = qr_bob.qr().unwrap().clone();
    qr_alice.apply(ExchangeEvent::ProcessQR(qr_b)).unwrap();
    qr_bob.apply(ExchangeEvent::ProcessQR(qr_a)).unwrap();
    qr_alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr_bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr_alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr_bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr_alice
        .apply(ExchangeEvent::CompleteExchange(bob_card.clone()))
        .unwrap();
    qr_bob
        .apply(ExchangeEvent::CompleteExchange(alice_card.clone()))
        .unwrap();

    let qr_key = match qr_alice.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().clone(),
        _ => panic!("QR Alice should be complete"),
    };

    // --- QR session 2 (separate identities, fresh ephemerals) ---
    let alice_identity2 = Identity::create("Alice", 0);
    let bob_identity2 = Identity::create("Bob", 0);

    let qr2_alice_proximity = MockProximityVerifier::success();
    let mut qr2_alice = ExchangeSession::new_qr(
        alice_identity2,
        alice_card.clone(),
        qr2_alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    let qr2_bob_proximity = MockProximityVerifier::success();
    let mut qr2_bob = ExchangeSession::new_qr(
        bob_identity2,
        bob_card.clone(),
        qr2_bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    qr2_alice.apply(ExchangeEvent::StartQR).unwrap();
    qr2_bob.apply(ExchangeEvent::StartQR).unwrap();
    let qr2_a = qr2_alice.qr().unwrap().clone();
    let qr2_b = qr2_bob.qr().unwrap().clone();
    qr2_alice.apply(ExchangeEvent::ProcessQR(qr2_b)).unwrap();
    qr2_bob.apply(ExchangeEvent::ProcessQR(qr2_a)).unwrap();
    qr2_alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr2_bob.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    qr2_alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr2_bob.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    qr2_alice
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    qr2_bob
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let qr2_key = match qr2_alice.state() {
        ExchangeState::Complete { contact } => contact.shared_key().unwrap().clone(),
        _ => panic!("QR2 Alice should be complete"),
    };

    // Sessions with fresh ephemerals must produce different secrets
    assert_ne!(
        qr_key.as_bytes(),
        qr2_key.as_bytes(),
        "Independent sessions must produce different shared secrets"
    );
}

// ============================================================
// EncryptedExchangeMessage ceremony: create + decrypt consistency
// ============================================================

/// Verify EncryptedExchangeMessage shared secret matches raw X3DH.
///
/// This cross-checks that the message layer uses X3DH internally
/// and produces the same key as calling X3DH::initiate/respond directly.
// @scenario: contact_exchange :: X3DH key agreement during exchange
// @internal
#[test]
fn test_encrypted_message_secret_matches_raw_x3dh() {
    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    let (msg, alice_secret) =
        EncryptedExchangeMessage::create(&alice, bob.public_key(), &[0x42u8; 32], "Alice").unwrap();

    let (_payload, bob_secret) = msg.decrypt(&bob).unwrap();

    // Both must match
    assert_eq!(
        alice_secret.as_bytes(),
        bob_secret.as_bytes(),
        "EncryptedExchangeMessage must produce matching secrets"
    );

    // The secret should be HKDF-derived (not raw DH)
    let raw_dh = bob
        .diffie_hellman(msg.ephemeral_public_key.as_bytes())
        .unwrap();
    assert_ne!(
        alice_secret.as_bytes(),
        raw_dh.as_ref(),
        "Secret must be HKDF-derived, not raw DH"
    );
}

// ============================================================
// Transcript binding tests (SP-2, item 76)
// ============================================================

/// Changing either identity key must produce a different shared secret.
///
/// This verifies that identity keys are bound into the key derivation,
/// preventing identity misbinding attacks where an attacker substitutes
/// their identity key while keeping the same DH output.
// @scenario: contact_exchange :: Exchange verifies identity
// @scenario: contact_exchange :: Identity mismatch detection
// @internal
#[test]
fn test_transcript_binding_includes_identity_keys() {
    let identity_a = Identity::create("Alice-A", 0);
    let identity_b = Identity::create("Alice-B", 0);
    let bob_identity = Identity::create("Bob", 0);

    // Use the same ephemeral X3DH keypair for both sessions
    let fixed_x3dh_seed = [0x42u8; 32];
    let fixed_x3dh_a = X3DHKeyPair::from_bytes(fixed_x3dh_seed);
    let fixed_x3dh_b = X3DHKeyPair::from_bytes(fixed_x3dh_seed);

    let card_a = ContactCard::new("Alice-A");
    let card_b = ContactCard::new("Alice-B");
    let bob_card = ContactCard::new("Bob");

    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Session A: identity_a + fixed ephemeral
    let prox_a = MockProximityVerifier::success();
    let mut session_a = ExchangeSession::new_qr_with_x3dh(
        identity_a,
        card_a,
        prox_a,
        fixed_x3dh_a,
        vauchi_core::clock::SystemClock::shared(),
    );
    session_a.apply(ExchangeEvent::StartQR).unwrap();
    session_a
        .apply(ExchangeEvent::ProcessQR(bob_qr.clone()))
        .unwrap();
    session_a.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    session_a.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    // Session B: identity_b + same fixed ephemeral
    let prox_b = MockProximityVerifier::success();
    let mut session_b = ExchangeSession::new_qr_with_x3dh(
        identity_b,
        card_b,
        prox_b,
        fixed_x3dh_b,
        vauchi_core::clock::SystemClock::shared(),
    );
    session_b.apply(ExchangeEvent::StartQR).unwrap();
    session_b
        .apply(ExchangeEvent::ProcessQR(bob_qr.clone()))
        .unwrap();
    session_b.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    session_b.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    // Extract shared keys from AwaitingCardExchange state
    let key_a = match session_a.state() {
        ExchangeState::AwaitingCardExchange { shared_key, .. } => shared_key.as_bytes().to_owned(),
        s => panic!("Session A: expected AwaitingCardExchange, got {:?}", s),
    };
    let key_b = match session_b.state() {
        ExchangeState::AwaitingCardExchange { shared_key, .. } => shared_key.as_bytes().to_owned(),
        s => panic!("Session B: expected AwaitingCardExchange, got {:?}", s),
    };

    // v2 transcript binding: different identity → different derived key
    assert_ne!(
        key_a, key_b,
        "Different identity keys must produce different shared secrets (transcript binding)"
    );
}

/// Changing either ephemeral key must produce a different shared secret,
/// even when identity keys are identical.
// @scenario: contact_exchange :: Mutual QR uses fresh ephemeral keys for forward secrecy
// @internal
#[test]
fn test_transcript_binding_includes_ephemeral_keys() {
    let alice_identity_1 = Identity::create("Alice", 0);
    let alice_identity_2 = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let x3dh_1 = X3DHKeyPair::from_bytes([0x01u8; 32]);
    let x3dh_2 = X3DHKeyPair::from_bytes([0x02u8; 32]);

    let card_1 = ContactCard::new("Alice");
    let card_2 = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let bob_prox = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        bob_prox,
        vauchi_core::clock::SystemClock::shared(),
    );
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    let bob_qr = bob_session.qr().unwrap().clone();

    // Session 1
    let prox_1 = MockProximityVerifier::success();
    let mut session_1 = ExchangeSession::new_qr_with_x3dh(
        alice_identity_1,
        card_1,
        prox_1,
        x3dh_1,
        vauchi_core::clock::SystemClock::shared(),
    );
    session_1.apply(ExchangeEvent::StartQR).unwrap();
    session_1
        .apply(ExchangeEvent::ProcessQR(bob_qr.clone()))
        .unwrap();
    session_1.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    session_1.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    // Session 2
    let prox_2 = MockProximityVerifier::success();
    let mut session_2 = ExchangeSession::new_qr_with_x3dh(
        alice_identity_2,
        card_2,
        prox_2,
        x3dh_2,
        vauchi_core::clock::SystemClock::shared(),
    );
    session_2.apply(ExchangeEvent::StartQR).unwrap();
    session_2
        .apply(ExchangeEvent::ProcessQR(bob_qr.clone()))
        .unwrap();
    session_2.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    session_2.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    let key_1 = match session_1.state() {
        ExchangeState::AwaitingCardExchange { shared_key, .. } => shared_key.as_bytes().to_owned(),
        s => panic!("Session 1: expected AwaitingCardExchange, got {:?}", s),
    };
    let key_2 = match session_2.state() {
        ExchangeState::AwaitingCardExchange { shared_key, .. } => shared_key.as_bytes().to_owned(),
        s => panic!("Session 2: expected AwaitingCardExchange, got {:?}", s),
    };

    assert_ne!(
        key_1, key_2,
        "Different ephemeral keys must produce different shared secrets"
    );
}

/// v2 domain tag must be incompatible with v1 for the same DH output.
/// This ensures the protocol upgrade is a clean break.
// @internal
#[test]
fn test_v2_domain_incompatible_with_v1() {
    use vauchi_core::crypto::HKDF;

    let shared_bytes = [0x99u8; 32];
    let identity_pk = [0x01u8; 32];
    let their_pk = [0x02u8; 32];
    let our_ephemeral = [0x03u8; 32];
    let their_ephemeral = [0x04u8; 32];

    // v1 derivation
    let v1_key = HKDF::derive_key(None, &shared_bytes, b"vauchi-x3dh-symmetric-v1");

    // v2 derivation (with transcript binding)
    let mut info_v2 = b"vauchi-x3dh-symmetric-v2".to_vec();
    info_v2.extend_from_slice(&identity_pk);
    info_v2.extend_from_slice(&their_pk);
    info_v2.extend_from_slice(&our_ephemeral);
    info_v2.extend_from_slice(&their_ephemeral);
    let v2_key = HKDF::derive_key(None, &shared_bytes, &info_v2);

    assert_ne!(
        v1_key, v2_key,
        "v2 derivation must differ from v1 for the same DH output"
    );
}

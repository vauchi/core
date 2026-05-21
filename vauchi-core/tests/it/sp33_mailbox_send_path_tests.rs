// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! SP-33 Task 4.1–4.4: Mailbox token send/receive path tests.
//!
//! Tests that the send path uses mailbox tokens as recipient_id,
//! that the client registers mailbox tokens after connect,
//! that device sync uses self-token EncryptedUpdate, and
//! that IdentityRevoked verification works correctly.

use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::network::mailbox_token::{
    compute_mailbox_token, compute_self_token, current_day_epoch, token_hex,
};
use vauchi_core::network::message::IdentityRevoked;
use vauchi_core::network::*;

fn create_test_config() -> RelayClientConfig {
    RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 100,
        max_retries: 3,
        delivery_receipts_enabled: true,
        suppress_presence: false,
    }
}

fn create_test_ratchet() -> (DoubleRatchetState, DoubleRatchetState) {
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let alice =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
    let bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);
    (alice, bob)
}

// ============================================================
// Task 4.1: Send path uses mailbox token as recipient_id
// ============================================================

/// When send_update is called with a mailbox-token-derived recipient_id,
/// the EncryptedUpdate on the wire must carry that token (64-char hex).
// @internal
#[test]
fn test_send_update_recipient_id_is_64_char_hex_token() {
    let shared_key = [0x42u8; 32];
    let token = compute_mailbox_token(&shared_key, current_day_epoch(0));
    let recipient_id = token_hex(&token);

    assert_eq!(recipient_id.len(), 64, "Mailbox token must be 64 hex chars");

    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut ratchet, _) = create_test_ratchet();
    let _msg_id = client
        .send_update(
            0,
            &recipient_id,
            &mut ratchet,
            b"hello",
            "update-1",
            Some(&shared_key),
        )
        .unwrap();

    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::EncryptedUpdate(update) = &sent[0].payload {
        assert_eq!(
            update.recipient_id, recipient_id,
            "Wire recipient_id must be the mailbox token"
        );
        assert_eq!(
            update.recipient_id.as_str().len(),
            64,
            "recipient_id must be 64-char hex"
        );
        // Must NOT be the sender's identity fingerprint
        assert_ne!(
            update.recipient_id, "sender-id",
            "recipient_id must not be identity fingerprint"
        );
    } else {
        panic!("Expected EncryptedUpdate payload");
    }
}

/// Negative case: if no shared key, send_update uses the provided recipient_id as-is.
// @internal
#[test]
fn test_send_update_without_shared_key_uses_provided_recipient() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let (mut ratchet, _) = create_test_ratchet();
    let _msg_id = client
        .send_update(
            0,
            "plain-contact-id",
            &mut ratchet,
            b"hello",
            "update-1",
            None,
        )
        .unwrap();

    let sent = client.connection().transport().sent_messages();
    if let MessagePayload::EncryptedUpdate(update) = &sent[0].payload {
        assert_eq!(update.recipient_id, "plain-contact-id");
    } else {
        panic!("Expected EncryptedUpdate");
    }
}

// ============================================================
// Task 4.2: Client sends RegisterMailbox on connect
// ============================================================

// @internal
#[test]
fn test_register_mailbox_tokens_sends_256_tokens() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let master_seed = [0xAA; 32];
    let contact_keys = [[0xBB; 32], [0xCC; 32]];

    let msg_id = client
        .register_mailbox_tokens(
            &contact_keys,
            &master_seed,
            0,
            0,
            &vauchi_core::rng::OsSecureRng::new(),
        )
        .unwrap();
    assert!(!msg_id.is_empty());

    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::RegisterMailbox(rm) = &sent[0].payload {
        assert_eq!(rm.tokens.len(), 256, "Must send exactly 256 padded tokens");

        // Self-token for today must be present
        let day = current_day_epoch(0);
        let self_token = token_hex(&compute_self_token(&master_seed, day));
        assert!(
            rm.tokens.contains(&self_token),
            "Self-token for today must be in registration batch"
        );

        // Contact tokens must be present
        for key in &contact_keys {
            let contact_token = token_hex(&compute_mailbox_token(key, day));
            assert!(
                rm.tokens.contains(&contact_token),
                "Contact token must be in registration batch"
            );
        }
    } else {
        panic!("Expected RegisterMailbox payload");
    }
}

// @internal
#[test]
fn test_register_mailbox_tokens_with_no_contacts() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let master_seed = [0xDD; 32];
    let msg_id = client
        .register_mailbox_tokens(
            &[],
            &master_seed,
            0,
            0,
            &vauchi_core::rng::OsSecureRng::new(),
        )
        .unwrap();
    assert!(!msg_id.is_empty());

    let sent = client.connection().transport().sent_messages();
    if let MessagePayload::RegisterMailbox(rm) = &sent[0].payload {
        assert_eq!(
            rm.tokens.len(),
            256,
            "Must pad to 256 even with zero contacts"
        );
    } else {
        panic!("Expected RegisterMailbox");
    }
}

// ============================================================
// Task 4.3: Device sync via self-token EncryptedUpdate
// ============================================================

// @internal
#[test]
fn test_device_sync_uses_self_token_as_recipient_id() {
    let transport = MockTransport::new();
    let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
    client.connect().unwrap();

    let master_seed = [0xEE; 32];
    let (mut ratchet, _) = create_test_ratchet();
    let ciphertext = b"encrypted-sync-payload".to_vec();
    let ratchet_msg = ratchet.encrypt(&ciphertext).unwrap();

    let msg_id = client
        .send_device_sync_message(&master_seed, ciphertext.clone(), &ratchet_msg, 0)
        .unwrap();
    assert!(!msg_id.is_empty());

    let sent = client.connection().transport().sent_messages();
    assert_eq!(sent.len(), 1);

    if let MessagePayload::EncryptedUpdate(update) = &sent[0].payload {
        let expected_token = token_hex(&compute_self_token(&master_seed, current_day_epoch(0)));
        assert_eq!(
            update.recipient_id, expected_token,
            "Device sync recipient_id must be the self-token"
        );
        assert_eq!(
            update.recipient_id.as_str().len(),
            64,
            "Self-token must be 64-char hex"
        );
    } else {
        panic!("Expected EncryptedUpdate for device sync");
    }
}

// ============================================================
// Task 4.4: IdentityRevoked Ed25519 verification (client-side)
// ============================================================

/// Valid IdentityRevoked signature is accepted.
// @internal
#[test]
fn test_identity_revoked_valid_signature_accepted() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    assert!(
        revoked.verify(identity.signing_public_key()),
        "Valid signature must be accepted"
    );
}

/// Forged signature is rejected (CC-14 adversarial).
// @internal
#[test]
fn test_identity_revoked_forged_signature_rejected() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let mut revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    // Tamper with the signature
    revoked.signature[0] ^= 0xFF;
    revoked.signature[63] ^= 0xFF;

    assert!(
        !revoked.verify(identity.signing_public_key()),
        "Forged signature must be rejected"
    );
}

/// Unknown sender's public key fails verification (CC-14 adversarial).
// @internal
#[test]
fn test_identity_revoked_unknown_sender_rejected() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let revoked = IdentityRevoked::create(&alice, &recipient_id, 1700000000);

    // Verify with Bob's key — should fail
    assert!(
        !revoked.verify(bob.signing_public_key()),
        "Verification with wrong public key must fail"
    );
}

/// Tampered timestamp makes signature invalid (CC-14 adversarial).
// @internal
#[test]
fn test_identity_revoked_tampered_timestamp_rejected() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let mut revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    // Tamper with the timestamp
    revoked.timestamp += 1;

    assert!(
        !revoked.verify(identity.signing_public_key()),
        "Tampered timestamp must invalidate signature"
    );
}

/// Tampered recipient_id makes signature invalid (CC-14 adversarial).
// @internal
#[test]
fn test_identity_revoked_tampered_recipient_rejected() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let mut revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    // Tamper with recipient
    revoked.recipient_id = hex::encode([0xCC; 32]).into();

    assert!(
        !revoked.verify(identity.signing_public_key()),
        "Tampered recipient_id must invalidate signature"
    );
}

/// Empty/zero public key fails verification.
// @internal
#[test]
fn test_identity_revoked_zero_pubkey_rejected() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xBB; 32]);

    let revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    assert!(
        !revoked.verify(&[0u8; 32]),
        "Zero public key must fail verification"
    );
}

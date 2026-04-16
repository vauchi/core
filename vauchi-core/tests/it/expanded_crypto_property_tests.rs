// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded Property-Based Tests for Crypto Operations
//!
//! Covers:
//! 1. Backup roundtrip with adversarial passwords (unicode, max-length, special chars)
//! 2. Key derivation determinism (Argon2id and HKDF)
//! 3. Protocol message serialization roundtrips (all MessagePayload variants)
//! 4. Out-of-order delivery tolerance (Double Ratchet skipped keys)

use proptest::prelude::*;

use vauchi_core::crypto::{DoubleRatchetState, HKDF, SymmetricKey, derive_key_argon2id};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::network::{
    AckStatus, Acknowledgment, DeletionStage, EncryptedUpdate, ForwardingHint, ForwardingHints,
    Handshake, IdentityDeletionNotice, IdentityRevoked, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, PresenceStatus, PresenceUpdate, PurgeRequest, RatchetHeader,
    VersionNegotiation,
};

// ============================================================
// Custom Strategies
// ============================================================

/// Strategy for generating 32-byte arrays (keys, IDs).
/// Filters out all-zeros since SymmetricKey::from_bytes rejects degenerate keys.
fn bytes32_strategy() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
        .prop_filter("all-zeros rejected", |b| b.iter().any(|&x| x != 0))
}

/// Strategy for strong passwords that pass zxcvbn score >= 3.
///
/// The password policy requires >= 8 chars and zxcvbn score >= 3.
/// We generate passwords by combining random words with special characters,
/// which consistently score >= 3 in zxcvbn.
fn strong_password_strategy() -> impl Strategy<Value = String> {
    // Three random words + special chars => always passes zxcvbn >= 3
    (
        "[a-z]{4,8}",
        "[A-Z]{2,4}",
        "[0-9]{2,4}",
        prop::sample::select(vec!["!@#", "$%^", "&*-", "+_=", "?<>"]),
    )
        .prop_map(|(word1, word2, digits, special)| {
            format!("{}-{}-{}{}", word1, word2, digits, special)
        })
}

/// Strategy for adversarial strong passwords with unicode and special characters.
///
/// These passwords are designed to stress-test the backup/restore pipeline
/// while still passing the zxcvbn >= 3 strength requirement.
fn adversarial_password_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Unicode passwords with sufficient entropy
        Just("Bücher-Straße-42!@#".to_string()),
        Just("日本語パスワード-Secure99!".to_string()),
        Just("Пароль-Надёжный-2026!".to_string()),
        Just("كلمة-المرور-الآمنة-42!".to_string()),
        Just("emoji-🔐🔑🛡️-Secure42!".to_string()),
        Just("MixedÜñíçödé-Str0ng!".to_string()),
        // Max-length password (128 chars)
        Just("A".repeat(60) + "-Str0ng-P@ssw0rd-" + &"B".repeat(60) + "!1"),
        // Passwords with null bytes in the middle (as string, not binary)
        Just("Secure-Pass\u{0000}word-42!@#Strong".to_string()),
        // RTL + LTR mixed
        Just("Hello-مرحبا-World-عالم-42!Strong".to_string()),
        // Combining diacriticals
        Just("Tes\u{0301}t-Pa\u{0308}ss-Wo\u{0327}rd-42!@#".to_string()),
    ]
}

/// Strategy for generating valid message IDs
fn message_id_strategy() -> impl Strategy<Value = String> {
    "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}"
}

/// Strategy for generating timestamps
fn timestamp_strategy() -> impl Strategy<Value = u64> {
    1000000000u64..2000000000u64
}

/// Strategy for generating hex-encoded 32-byte IDs (like public key fingerprints)
fn hex_id_strategy() -> impl Strategy<Value = String> {
    "[a-f0-9]{64}"
}

// ============================================================
// 1. Backup Roundtrip with Adversarial Passwords
// ============================================================

// Argon2id (m=64MB, t=3, p=4) ~300ms per KDF call, 2 calls per case.
// 3 cases is sufficient to cover the adversarial password set variation.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(3))]

    /// Property: Backup/restore roundtrip preserves identity for adversarial passwords.
    ///
    /// Tests that the backup pipeline handles unicode, combining characters,
    /// null bytes, RTL text, emoji, and max-length passwords correctly.
// @internal
    #[test]
    fn prop_backup_roundtrip_adversarial_passwords(
        password in adversarial_password_strategy()
    ) {
        let original = Identity::create("Adversarial Test User");

        let backup = original.export_backup(&password)
            .unwrap_or_else(|e| panic!("export_backup should succeed for password: {:?}: {}", password, e));
        let restored = Identity::import_backup(&backup, &password)
            .unwrap_or_else(|e| panic!("import_backup should succeed for password: {:?}: {}", password, e));

        // Verify cryptographic identity is preserved
        prop_assert_eq!(
            original.signing_public_key(),
            restored.signing_public_key(),
            "Signing public key must survive backup roundtrip"
        );
        prop_assert_eq!(
            original.public_id(),
            restored.public_id(),
            "Public ID must survive backup roundtrip"
        );
        prop_assert_eq!(
            original.exchange_public_key(),
            restored.exchange_public_key(),
            "Exchange public key must survive backup roundtrip"
        );
    }

    /// Property: Backup/restore roundtrip preserves identity for randomly-generated strong passwords.
// @internal
    #[test]
    fn prop_backup_roundtrip_random_strong_passwords(
        password in strong_password_strategy()
    ) {
        let original = Identity::create("Random Password Test");

        let backup = original.export_backup(&password)
            .unwrap_or_else(|e| panic!("export_backup should succeed for password: {:?}: {}", password, e));
        let restored = Identity::import_backup(&backup, &password)
            .unwrap_or_else(|e| panic!("import_backup should succeed for password: {:?}: {}", password, e));

        prop_assert_eq!(
            original.signing_public_key(),
            restored.signing_public_key(),
            "Signing public key must survive roundtrip"
        );
        prop_assert_eq!(
            original.public_id(),
            restored.public_id(),
            "Public ID must survive roundtrip"
        );
    }

    /// Property: Wrong password always fails backup restore.
// @internal
    #[test]
    fn prop_backup_wrong_password_fails(
        password1 in adversarial_password_strategy(),
        password2 in adversarial_password_strategy()
    ) {
        prop_assume!(password1 != password2);

        let original = Identity::create("Wrong Password Test");
        let backup = original.export_backup(&password1).expect("export should succeed");

        let result = Identity::import_backup(&backup, &password2);
        prop_assert!(
            result.is_err(),
            "Restoring with wrong password must fail"
        );
    }

    /// Property: Tampered backup data fails restore.
// @internal
    #[test]
    fn prop_backup_tampered_data_fails(
        password in adversarial_password_strategy(),
        tamper_offset in 17usize..100usize, // Skip version byte and salt
        tamper_byte in any::<u8>()
    ) {
        let original = Identity::create("Tamper Test User");
        let mut backup = original.export_backup(&password).expect("export should succeed");

        let data = backup.as_bytes_mut();
        if tamper_offset < data.len() {
            let original_byte = data[tamper_offset];
            prop_assume!(tamper_byte != original_byte);
            data[tamper_offset] = tamper_byte;

            let result = Identity::import_backup(&backup, &password);
            prop_assert!(
                result.is_err(),
                "Restoring tampered backup must fail"
            );
        }
    }
}

// ============================================================
// 2. Key Derivation Determinism
// ============================================================

// Argon2id is expensive; 3 cases is enough to verify determinism.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(3))]

    /// Property: Argon2id key derivation is deterministic.
    ///
    /// The same (password, salt) pair must always produce the same derived key.
// @internal
    #[test]
    fn prop_argon2id_deterministic(
        password in prop::collection::vec(any::<u8>(), 8..64),
        salt in prop::collection::vec(any::<u8>(), 16..32)
    ) {
        let key1 = derive_key_argon2id(&password, &salt)
            .expect("Argon2id derivation should succeed");
        let key2 = derive_key_argon2id(&password, &salt)
            .expect("Argon2id derivation should succeed");

        prop_assert_eq!(
            key1.as_bytes(),
            key2.as_bytes(),
            "Same inputs must produce same derived key"
        );
    }

    /// Property: Different salts produce different Argon2id keys.
// @internal
    #[test]
    fn prop_argon2id_different_salts_different_keys(
        password in prop::collection::vec(any::<u8>(), 8..64),
        salt1 in prop::collection::vec(any::<u8>(), 16..32),
        salt2 in prop::collection::vec(any::<u8>(), 16..32)
    ) {
        prop_assume!(salt1 != salt2);

        let key1 = derive_key_argon2id(&password, &salt1)
            .expect("Argon2id derivation should succeed");
        let key2 = derive_key_argon2id(&password, &salt2)
            .expect("Argon2id derivation should succeed");

        prop_assert_ne!(
            key1.as_bytes(),
            key2.as_bytes(),
            "Different salts must produce different keys"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: HKDF derivation is deterministic.
    ///
    /// The same (salt, ikm, info) must always produce the same output.
// @internal
    #[test]
    fn prop_hkdf_deterministic(
        salt in prop::collection::vec(any::<u8>(), 16..64),
        ikm in prop::collection::vec(any::<u8>(), 16..64),
        info in prop::collection::vec(any::<u8>(), 0..32),
        length in 1usize..128
    ) {
        let result1 = HKDF::derive(Some(&salt), &ikm, &info, length)
            .expect("HKDF derive should succeed");
        let result2 = HKDF::derive(Some(&salt), &ikm, &info, length)
            .expect("HKDF derive should succeed");

        prop_assert_eq!(
            result1, result2,
            "Same inputs must produce same HKDF output"
        );
    }

    /// Property: HKDF derive_key is deterministic and produces 32 bytes.
// @internal
    #[test]
    fn prop_hkdf_derive_key_deterministic(
        salt in prop::collection::vec(any::<u8>(), 16..64),
        ikm in prop::collection::vec(any::<u8>(), 16..64),
        info in prop::collection::vec(any::<u8>(), 0..32)
    ) {
        let key1 = HKDF::derive_key(Some(&salt), &ikm, &info);
        let key2 = HKDF::derive_key(Some(&salt), &ikm, &info);

        prop_assert_eq!(key1, key2, "Same inputs must produce same HKDF key");
    }

    /// Property: HKDF derive_key_pair is deterministic and produces two distinct keys.
// @internal
    #[test]
    fn prop_hkdf_derive_key_pair_deterministic(
        salt in prop::collection::vec(any::<u8>(), 16..64),
        ikm in prop::collection::vec(any::<u8>(), 16..64),
        info in prop::collection::vec(any::<u8>(), 0..32)
    ) {
        let (k1a, k1b) = HKDF::derive_key_pair(Some(&salt), &ikm, &info);
        let (k2a, k2b) = HKDF::derive_key_pair(Some(&salt), &ikm, &info);

        prop_assert_eq!(*k1a, *k2a, "First key must be deterministic");
        prop_assert_eq!(*k1b, *k2b, "Second key must be deterministic");
        // The two keys in a pair should be different (overwhelmingly likely)
        prop_assert_ne!(*k1a, *k1b, "Key pair should produce two distinct keys");
    }

    /// Property: Different IKM produces different HKDF output.
// @internal
    #[test]
    fn prop_hkdf_different_ikm_different_output(
        ikm1 in prop::collection::vec(any::<u8>(), 16..64),
        ikm2 in prop::collection::vec(any::<u8>(), 16..64),
        salt in prop::collection::vec(any::<u8>(), 16..64),
        info in prop::collection::vec(any::<u8>(), 0..32)
    ) {
        prop_assume!(ikm1 != ikm2);

        let result1 = HKDF::derive(Some(&salt), &ikm1, &info, 32)
            .expect("HKDF derive should succeed");
        let result2 = HKDF::derive(Some(&salt), &ikm2, &info, 32)
            .expect("HKDF derive should succeed");

        prop_assert_ne!(
            result1, result2,
            "Different IKM must produce different HKDF output"
        );
    }
}

// ============================================================
// 3. Protocol Message Serialization Roundtrips
// ============================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: EncryptedUpdate payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_encrypted_update_roundtrip(
        recipient_id in hex_id_strategy(),
        sender_id in hex_id_strategy(),
        dh_public in bytes32_strategy(),
        dh_generation in any::<u32>(),
        message_index in any::<u32>(),
        previous_chain_length in any::<u32>(),
        ciphertext in prop::collection::vec(any::<u8>(), 1..200),
        msg_id in message_id_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp,
            payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
                recipient_id: recipient_id.clone(),
                sender_id: sender_id.clone(),
                ratchet_header: RatchetHeader {
                    dh_public,
                    dh_generation,
                    message_index,
                    previous_chain_length,
                },
                ciphertext: ciphertext.clone(),
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        prop_assert_eq!(restored.version, PROTOCOL_VERSION);
        prop_assert_eq!(&restored.message_id, &msg_id);
        prop_assert_eq!(restored.timestamp, timestamp);

        if let MessagePayload::EncryptedUpdate(update) = &restored.payload {
            prop_assert_eq!(&update.recipient_id, &recipient_id);
            prop_assert_eq!(&update.sender_id, &sender_id);
            prop_assert_eq!(update.ratchet_header.dh_public, dh_public);
            prop_assert_eq!(update.ratchet_header.dh_generation, dh_generation);
            prop_assert_eq!(update.ratchet_header.message_index, message_index);
            prop_assert_eq!(update.ratchet_header.previous_chain_length, previous_chain_length);
            prop_assert_eq!(&update.ciphertext, &ciphertext);
        } else {
            prop_assert!(false, "Expected EncryptedUpdate variant");
        }
    }

    /// Property: Acknowledgment payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_acknowledgment_roundtrip(
        ack_msg_id in message_id_strategy(),
        status in prop_oneof![
            Just(AckStatus::Stored),
            Just(AckStatus::Delivered),
            Just(AckStatus::ReceivedByRecipient),
            Just(AckStatus::Failed),
        ],
        error_msg in proptest::option::of("[a-zA-Z0-9 ]{1,50}"),
        msg_id in message_id_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp,
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: ack_msg_id.clone(),
                status,
                error: error_msg.clone(),
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        prop_assert_eq!(&restored.message_id, &msg_id);
        if let MessagePayload::Acknowledgment(ack) = &restored.payload {
            prop_assert_eq!(&ack.message_id, &ack_msg_id);
            prop_assert_eq!(ack.status, status);
            prop_assert_eq!(&ack.error, &error_msg);
        } else {
            prop_assert!(false, "Expected Acknowledgment variant");
        }
    }

    /// Property: Handshake payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_handshake_roundtrip(
        identity_key in bytes32_strategy(),
        nonce in bytes32_strategy(),
        signature in prop::array::uniform32(any::<u8>()).prop_flat_map(|first| {
            prop::array::uniform32(any::<u8>()).prop_map(move |second| {
                let mut sig = [0u8; 64];
                sig[..32].copy_from_slice(&first);
                sig[32..].copy_from_slice(&second);
                sig
            })
        }),
        msg_id in message_id_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp,
            payload: MessagePayload::Handshake(Handshake {
                identity_public_key: identity_key,
                nonce,
                signature,
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::Handshake(h) = &restored.payload {
            prop_assert_eq!(h.identity_public_key, identity_key);
            prop_assert_eq!(h.nonce, nonce);
            prop_assert_eq!(h.signature, signature);
        } else {
            prop_assert!(false, "Expected Handshake variant");
        }
    }

    /// Property: Presence payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_presence_roundtrip(
        status in prop_oneof![
            Just(PresenceStatus::Online),
            Just(PresenceStatus::Away),
            Just(PresenceStatus::Offline),
        ],
        message in proptest::option::of("[a-zA-Z0-9 ]{1,100}"),
        msg_id in message_id_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp,
            payload: MessagePayload::Presence(PresenceUpdate {
                status,
                message: message.clone(),
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::Presence(p) = &restored.payload {
            prop_assert_eq!(p.status, status);
            prop_assert_eq!(&p.message, &message);
        } else {
            prop_assert!(false, "Expected Presence variant");
        }
    }

    // DeviceSync roundtrip property test removed (SP-33): wire type removed.

    /// Property: IdentityRevoked payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_identity_revoked_roundtrip(
        sender_id in hex_id_strategy(),
        recipient_id in hex_id_strategy(),
        timestamp in timestamp_strategy(),
        signature in prop::array::uniform32(any::<u8>()).prop_flat_map(|first| {
            prop::array::uniform32(any::<u8>()).prop_map(move |second| {
                let mut sig = [0u8; 64];
                sig[..32].copy_from_slice(&first);
                sig[32..].copy_from_slice(&second);
                sig
            })
        }),
        msg_id in message_id_strategy(),
        env_timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp: env_timestamp,
            payload: MessagePayload::IdentityRevoked(IdentityRevoked {
                sender_id: sender_id.clone(),
                recipient_id: recipient_id.clone(),
                timestamp,
                signature,
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::IdentityRevoked(r) = &restored.payload {
            prop_assert_eq!(&r.sender_id, &sender_id);
            prop_assert_eq!(&r.recipient_id, &recipient_id);
            prop_assert_eq!(r.timestamp, timestamp);
            prop_assert_eq!(r.signature, signature);
        } else {
            prop_assert!(false, "Expected IdentityRevoked variant");
        }
    }

    /// Property: IdentityDeletionNotice payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_identity_deletion_notice_roundtrip(
        stage in prop_oneof![
            Just(DeletionStage::Pending),
            Just(DeletionStage::Confirmed),
            Just(DeletionStage::Cancelled),
        ],
        public_key in bytes32_strategy(),
        timestamp in timestamp_strategy(),
        signature in prop::array::uniform32(any::<u8>()).prop_flat_map(|first| {
            prop::array::uniform32(any::<u8>()).prop_map(move |second| {
                let mut sig = [0u8; 64];
                sig[..32].copy_from_slice(&first);
                sig[32..].copy_from_slice(&second);
                sig
            })
        }),
        msg_id in message_id_strategy(),
        env_timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp: env_timestamp,
            payload: MessagePayload::IdentityDeletionNotice(IdentityDeletionNotice {
                stage,
                public_key,
                timestamp,
                signature,
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::IdentityDeletionNotice(n) = &restored.payload {
            prop_assert_eq!(n.stage, stage);
            prop_assert_eq!(n.public_key, public_key);
            prop_assert_eq!(n.timestamp, timestamp);
            prop_assert_eq!(n.signature, signature);
        } else {
            prop_assert!(false, "Expected IdentityDeletionNotice variant");
        }
    }

    /// Property: PurgeRequest payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_purge_request_roundtrip(
        public_key in bytes32_strategy(),
        signature in prop::collection::vec(any::<u8>(), 64..65),
        purge_token in bytes32_strategy(),
        timestamp in timestamp_strategy(),
        msg_id in message_id_strategy(),
        env_timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp: env_timestamp,
            payload: MessagePayload::PurgeRequest(PurgeRequest {
                public_key,
                signature: signature.clone(),
                purge_token,
                timestamp,
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::PurgeRequest(r) = &restored.payload {
            prop_assert_eq!(r.public_key, public_key);
            prop_assert_eq!(&r.signature, &signature);
            prop_assert_eq!(r.purge_token, purge_token);
            prop_assert_eq!(r.timestamp, timestamp);
        } else {
            prop_assert!(false, "Expected PurgeRequest variant");
        }
    }

    /// Property: ForwardingHints payload roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_forwarding_hints_roundtrip(
        blob_id in "[a-f0-9]{16}",
        relay_url in "https://[a-z]{3,10}\\.[a-z]{2,4}/relay",
        expires_at in timestamp_strategy(),
        msg_id in message_id_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: msg_id.clone(),
            timestamp,
            payload: MessagePayload::ForwardingHints(ForwardingHints {
                hints: vec![ForwardingHint {
                    blob_id: blob_id.clone(),
                    relay_url: relay_url.clone(),
                    expires_at_secs: expires_at,
                }],
                relay_signing_key: None,
                signature: None,
            }),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let restored: MessageEnvelope = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        if let MessagePayload::ForwardingHints(h) = &restored.payload {
            prop_assert_eq!(h.hints.len(), 1);
            prop_assert_eq!(&h.hints[0].blob_id, &blob_id);
            prop_assert_eq!(&h.hints[0].relay_url, &relay_url);
            prop_assert_eq!(h.hints[0].expires_at_secs, expires_at);
        } else {
            prop_assert!(false, "Expected ForwardingHints variant");
        }
    }

    /// Property: VersionNegotiation roundtrips through JSON serialization.
// @internal
    #[test]
    fn prop_version_negotiation_roundtrip(
        versions in prop::collection::vec(1u32..100, 1..10),
        preferred in 1u32..100
    ) {
        let vn = VersionNegotiation {
            supported_versions: versions.clone(),
            preferred_version: preferred,
        };

        let json = serde_json::to_string(&vn).expect("serialization should succeed");
        let restored: VersionNegotiation = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        prop_assert_eq!(restored.supported_versions, versions);
        prop_assert_eq!(restored.preferred_version, preferred);
    }
}

// ============================================================
// 4. Out-of-Order Delivery Tolerance
// ============================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Property: Messages encrypted in order can be decrypted out-of-order.
    ///
    /// The Double Ratchet stores skipped message keys, allowing messages
    /// received out of order to be decrypted correctly.
// @internal
    #[test]
    fn prop_ratchet_out_of_order_delivery(
        seed in bytes32_strategy(),
        message_count in 3usize..8
    ) {
        let shared_secret = SymmetricKey::from_bytes(seed);
        let bob_dh = X3DHKeyPair::generate();

        let mut alice_ratchet =
            DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
        let mut bob_ratchet =
            DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        // Alice encrypts several messages in order
        let mut encrypted_messages: Vec<(usize, _)> = Vec::new();
        let mut plaintexts: Vec<Vec<u8>> = Vec::new();

        for i in 0..message_count {
            let plaintext = format!("Message {}", i).into_bytes();
            let encrypted = alice_ratchet.encrypt(&plaintext)
                .expect("encryption should succeed");
            encrypted_messages.push((i, encrypted));
            plaintexts.push(plaintext);
        }

        // Deliver in reverse order (worst case for out-of-order)
        encrypted_messages.reverse();

        for (original_index, encrypted) in &encrypted_messages {
            let decrypted = bob_ratchet.decrypt(encrypted)
                .unwrap_or_else(|e| panic!(
                    "decryption of message {} should succeed (out-of-order delivery): {}",
                    original_index, e
                ));
            prop_assert_eq!(
                &decrypted,
                &plaintexts[*original_index],
                "Decrypted content must match original for message {}",
                original_index
            );
        }
    }

    /// Property: Interleaved send/receive with skipped messages decrypts correctly.
    ///
    /// Tests that when Alice sends N messages but Bob receives them with gaps,
    /// the skipped messages can still be decrypted later.
// @internal
    #[test]
    fn prop_ratchet_skip_then_catch_up(
        seed in bytes32_strategy(),
        skip_count in 1usize..5,
        total_count in 5usize..10
    ) {
        let total = total_count.max(skip_count + 2);
        let shared_secret = SymmetricKey::from_bytes(seed);
        let bob_dh = X3DHKeyPair::generate();

        let mut alice_ratchet =
            DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
        let mut bob_ratchet =
            DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        // Alice encrypts all messages
        let mut messages = Vec::new();
        let mut expected_plaintexts = Vec::new();
        for i in 0..total {
            let plaintext = format!("Msg-{}", i).into_bytes();
            let encrypted = alice_ratchet.encrypt(&plaintext)
                .expect("encryption should succeed");
            messages.push(encrypted);
            expected_plaintexts.push(plaintext);
        }

        // Bob receives the last message first (skipping `skip_count` messages)
        let last_idx = total - 1;
        let decrypted_last = bob_ratchet.decrypt(&messages[last_idx])
            .expect("decrypting last message should succeed");
        prop_assert_eq!(
            &decrypted_last,
            &expected_plaintexts[last_idx],
            "Last message must decrypt correctly"
        );

        // Now Bob catches up on skipped messages (in order)
        for i in 0..last_idx {
            let decrypted = bob_ratchet.decrypt(&messages[i])
                .unwrap_or_else(|e| panic!("catching up message {} should succeed: {}", i, e));
            prop_assert_eq!(
                &decrypted,
                &expected_plaintexts[i],
                "Caught-up message {} must match",
                i
            );
        }
    }

    /// Property: Duplicate message decryption fails after first successful decrypt.
    ///
    /// Once a message has been decrypted, attempting to decrypt it again must fail
    /// (replay protection via consumed skipped keys).
// @internal
    #[test]
    fn prop_ratchet_duplicate_message_rejected(
        seed in bytes32_strategy()
    ) {
        let shared_secret = SymmetricKey::from_bytes(seed);
        let bob_dh = X3DHKeyPair::generate();

        let mut alice_ratchet =
            DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
        let mut bob_ratchet =
            DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        let plaintext = b"unique message";
        let encrypted = alice_ratchet.encrypt(plaintext)
            .expect("encryption should succeed");

        // First decryption succeeds
        let decrypted = bob_ratchet.decrypt(&encrypted)
            .expect("first decryption should succeed");
        prop_assert_eq!(decrypted.as_slice(), plaintext.as_slice());

        // Second decryption of same message must fail (replay protection)
        let duplicate_result = bob_ratchet.decrypt(&encrypted);
        prop_assert!(
            duplicate_result.is_err(),
            "Duplicate message decryption must fail"
        );
    }
}

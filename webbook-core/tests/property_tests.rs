//! Property-Based Tests
//!
//! Uses proptest to verify properties that should hold for all inputs,
//! not just specific test cases.

use proptest::prelude::*;
use std::collections::HashSet;

use webbook_core::contact::{FieldVisibility, VisibilityRules};
use webbook_core::contact_card::{ContactCard, ContactField, FieldType};
use webbook_core::crypto::{decrypt, encrypt, SigningKeyPair, SymmetricKey};
use webbook_core::identity::{DeviceInfo, Identity};
use webbook_core::sync::{SyncItem, VersionVector};

// ============================================================
// Custom Strategies for generating test data
// ============================================================

/// Strategy for generating valid display names (non-empty, reasonable length)
fn display_name_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,49}"
        .prop_map(|s| s.trim().to_string())
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating field labels
fn field_label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,19}"
}

/// Strategy for generating field values
fn field_value_strategy() -> impl Strategy<Value = String> {
    ".{1,100}"
}

/// Strategy for generating 32-byte arrays (keys, IDs)
fn bytes32_strategy() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
}

/// Strategy for generating timestamps
fn timestamp_strategy() -> impl Strategy<Value = u64> {
    1000000000u64..2000000000u64
}

// ============================================================
// Serialization Roundtrip Properties
// ============================================================

proptest! {
    /// Property: ContactCard JSON roundtrip preserves all data
    #[test]
    fn prop_contact_card_json_roundtrip(name in display_name_strategy()) {
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(card.display_name(), restored.display_name());
    }

    /// Property: ContactCard with fields JSON roundtrip
    #[test]
    fn prop_contact_card_with_fields_roundtrip(
        name in display_name_strategy(),
        label in field_label_strategy(),
        value in "[a-zA-Z0-9@.+]{1,50}"
    ) {
        let mut card = ContactCard::new(&name);
        let field = ContactField::new(FieldType::Custom, &label, &value);
        let _ = card.add_field(field);

        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(card.display_name(), restored.display_name());
        prop_assert_eq!(card.fields().len(), restored.fields().len());
    }

    /// Property: VisibilityRules JSON roundtrip
    #[test]
    fn prop_visibility_rules_roundtrip(
        field_id in field_label_strategy(),
        contact_id in "[a-f0-9]{64}"
    ) {
        let mut rules = VisibilityRules::new();
        let mut contacts = HashSet::new();
        contacts.insert(contact_id.clone());
        rules.set_contacts(&field_id, contacts);

        let json = serde_json::to_string(&rules).unwrap();
        let restored: VisibilityRules = serde_json::from_str(&json).unwrap();

        prop_assert!(restored.can_see(&field_id, &contact_id));
        prop_assert!(!restored.can_see(&field_id, "other_contact"));
    }

    /// Property: SyncItem JSON roundtrip preserves timestamp
    #[test]
    fn prop_sync_item_roundtrip(
        label in field_label_strategy(),
        value in field_value_strategy(),
        timestamp in timestamp_strategy()
    ) {
        let item = SyncItem::CardUpdated {
            field_label: label,
            new_value: value,
            timestamp,
        };

        let json = item.to_json();
        let restored = SyncItem::from_json(&json).unwrap();

        prop_assert_eq!(item.timestamp(), restored.timestamp());
    }

    /// Property: VersionVector increment preserves count
    /// (JSON roundtrip is problematic for HashMap<[u8;32],_>, so test behavior instead)
    #[test]
    fn prop_version_vector_increment_preserves(
        device_id in bytes32_strategy(),
        count in 1u64..100u64
    ) {
        let mut vv = VersionVector::new();
        for _ in 0..count {
            vv.increment(&device_id);
        }

        // Verify the count is preserved after all increments
        prop_assert_eq!(vv.get(&device_id), count);
    }
}

// ============================================================
// Cryptographic Properties
// ============================================================

proptest! {
    /// Property: Encryption/decryption is a perfect roundtrip
    #[test]
    fn prop_encryption_roundtrip(
        key_bytes in bytes32_strategy(),
        plaintext in prop::collection::vec(any::<u8>(), 1..1000)
    ) {
        let key = SymmetricKey::from_bytes(key_bytes);

        let ciphertext = encrypt(&key, &plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();

        prop_assert_eq!(plaintext, decrypted);
    }

    /// Property: Ciphertext is different from plaintext (for non-trivial input)
    #[test]
    fn prop_encryption_transforms_data(
        key_bytes in bytes32_strategy(),
        plaintext in prop::collection::vec(any::<u8>(), 32..100)
    ) {
        let key = SymmetricKey::from_bytes(key_bytes);

        let ciphertext = encrypt(&key, &plaintext).unwrap();

        // Ciphertext should be longer (nonce + tag) and different
        prop_assert!(ciphertext.len() > plaintext.len());
        prop_assert_ne!(ciphertext[..plaintext.len()].to_vec(), plaintext);
    }

    /// Property: Different keys produce different ciphertexts
    #[test]
    fn prop_different_keys_different_ciphertext(
        key1_bytes in bytes32_strategy(),
        key2_bytes in bytes32_strategy(),
        plaintext in prop::collection::vec(any::<u8>(), 32..100)
    ) {
        prop_assume!(key1_bytes != key2_bytes);

        let key1 = SymmetricKey::from_bytes(key1_bytes);
        let key2 = SymmetricKey::from_bytes(key2_bytes);

        let ciphertext1 = encrypt(&key1, &plaintext).unwrap();
        let ciphertext2 = encrypt(&key2, &plaintext).unwrap();

        // Due to random nonces, even same key produces different ciphertexts,
        // but definitely different keys should
        prop_assert_ne!(ciphertext1, ciphertext2);
    }

    /// Property: Decryption with wrong key fails
    #[test]
    fn prop_wrong_key_fails_decryption(
        key1_bytes in bytes32_strategy(),
        key2_bytes in bytes32_strategy(),
        plaintext in prop::collection::vec(any::<u8>(), 1..100)
    ) {
        prop_assume!(key1_bytes != key2_bytes);

        let key1 = SymmetricKey::from_bytes(key1_bytes);
        let key2 = SymmetricKey::from_bytes(key2_bytes);

        let ciphertext = encrypt(&key1, &plaintext).unwrap();
        let result = decrypt(&key2, &ciphertext);

        prop_assert!(result.is_err());
    }

    /// Property: Signing and verification roundtrip
    #[test]
    fn prop_signing_roundtrip(
        seed in bytes32_strategy(),
        message in prop::collection::vec(any::<u8>(), 1..1000)
    ) {
        let keypair = SigningKeyPair::from_seed(&seed);
        let signature = keypair.sign(&message);

        prop_assert!(keypair.public_key().verify(&message, &signature));
    }

    /// Property: Tampered message fails verification
    #[test]
    fn prop_tampered_message_fails_verification(
        seed in bytes32_strategy(),
        message in prop::collection::vec(any::<u8>(), 2..100),
        tamper_index in any::<prop::sample::Index>()
    ) {
        let keypair = SigningKeyPair::from_seed(&seed);
        let signature = keypair.sign(&message);

        // Tamper with the message
        let mut tampered = message.clone();
        let idx = tamper_index.index(tampered.len());
        tampered[idx] = tampered[idx].wrapping_add(1);

        prop_assert!(!keypair.public_key().verify(&tampered, &signature));
    }
}

// ============================================================
// Data Structure Invariants
// ============================================================

proptest! {
    /// Property: VersionVector increment always increases version
    #[test]
    fn prop_version_vector_increment_increases(
        device_id in bytes32_strategy(),
        initial_count in 0u64..50u64
    ) {
        let mut vv = VersionVector::new();

        // Set initial state
        for _ in 0..initial_count {
            vv.increment(&device_id);
        }

        let before = vv.get(&device_id);
        vv.increment(&device_id);
        let after = vv.get(&device_id);

        prop_assert_eq!(after, before + 1);
    }

    /// Property: VersionVector merge is commutative
    #[test]
    fn prop_version_vector_merge_commutative(
        device_a in bytes32_strategy(),
        device_b in bytes32_strategy(),
        count_a in 1u64..10u64,
        count_b in 1u64..10u64
    ) {
        let mut vv_a = VersionVector::new();
        let mut vv_b = VersionVector::new();

        for _ in 0..count_a {
            vv_a.increment(&device_a);
        }
        for _ in 0..count_b {
            vv_b.increment(&device_b);
        }

        let merged_ab = VersionVector::merge(&vv_a, &vv_b);
        let merged_ba = VersionVector::merge(&vv_b, &vv_a);

        prop_assert_eq!(merged_ab.get(&device_a), merged_ba.get(&device_a));
        prop_assert_eq!(merged_ab.get(&device_b), merged_ba.get(&device_b));
    }

    /// Property: VersionVector merge takes maximum
    #[test]
    fn prop_version_vector_merge_takes_max(
        device_id in bytes32_strategy(),
        count_a in 1u64..50u64,
        count_b in 1u64..50u64
    ) {
        let mut vv_a = VersionVector::new();
        let mut vv_b = VersionVector::new();

        for _ in 0..count_a {
            vv_a.increment(&device_id);
        }
        for _ in 0..count_b {
            vv_b.increment(&device_id);
        }

        let merged = VersionVector::merge(&vv_a, &vv_b);

        prop_assert_eq!(merged.get(&device_id), std::cmp::max(count_a, count_b));
    }

    /// Property: SyncItem conflict resolution - later timestamp wins
    #[test]
    fn prop_sync_item_later_wins(
        label in field_label_strategy(),
        value_a in field_value_strategy(),
        value_b in field_value_strategy(),
        ts_a in timestamp_strategy(),
        ts_b in timestamp_strategy()
    ) {
        let item_a = SyncItem::CardUpdated {
            field_label: label.clone(),
            new_value: value_a.clone(),
            timestamp: ts_a,
        };

        let item_b = SyncItem::CardUpdated {
            field_label: label,
            new_value: value_b.clone(),
            timestamp: ts_b,
        };

        let resolved = SyncItem::resolve_conflict(&item_a, &item_b);

        if ts_a >= ts_b {
            prop_assert_eq!(resolved.timestamp(), ts_a);
        } else {
            prop_assert_eq!(resolved.timestamp(), ts_b);
        }
    }

    /// Property: Device key derivation is deterministic
    #[test]
    fn prop_device_derivation_deterministic(
        seed in bytes32_strategy(),
        device_index in 0u32..100u32,
        name in display_name_strategy()
    ) {
        let device1 = DeviceInfo::derive(&seed, device_index, name.clone());
        let device2 = DeviceInfo::derive(&seed, device_index, name);

        prop_assert_eq!(device1.device_id(), device2.device_id());
        prop_assert_eq!(device1.exchange_public_key(), device2.exchange_public_key());
    }

    /// Property: Different device indices produce different keys
    #[test]
    fn prop_different_indices_different_keys(
        seed in bytes32_strategy(),
        index_a in 0u32..100u32,
        index_b in 0u32..100u32
    ) {
        prop_assume!(index_a != index_b);

        let device_a = DeviceInfo::derive(&seed, index_a, "Device A".to_string());
        let device_b = DeviceInfo::derive(&seed, index_b, "Device B".to_string());

        prop_assert_ne!(device_a.device_id(), device_b.device_id());
        prop_assert_ne!(device_a.exchange_public_key(), device_b.exchange_public_key());
    }

    /// Property: Identity backup/restore preserves public keys
    #[test]
    fn prop_identity_backup_restore(
        name in display_name_strategy()
    ) {
        let password = "SecurePassword123!";
        let original = Identity::create(&name);

        let backup = original.export_backup(password).unwrap();
        let restored = Identity::import_backup(&backup, password).unwrap();

        prop_assert_eq!(original.signing_public_key(), restored.signing_public_key());
        prop_assert_eq!(original.public_id(), restored.public_id());
    }
}

// ============================================================
// Field Visibility Properties
// ============================================================

proptest! {
    /// Property: Everyone visibility allows all contacts
    #[test]
    fn prop_everyone_allows_all(
        field_id in field_label_strategy(),
        contact_id in "[a-f0-9]{64}"
    ) {
        let mut rules = VisibilityRules::new();
        rules.set_everyone(&field_id);

        prop_assert!(rules.can_see(&field_id, &contact_id));
    }

    /// Property: Nobody visibility blocks all contacts
    #[test]
    fn prop_nobody_blocks_all(
        field_id in field_label_strategy(),
        contact_id in "[a-f0-9]{64}"
    ) {
        let mut rules = VisibilityRules::new();
        rules.set_nobody(&field_id);

        prop_assert!(!rules.can_see(&field_id, &contact_id));
    }

    /// Property: Contacts visibility is precise
    #[test]
    fn prop_contacts_visibility_precise(
        field_id in field_label_strategy(),
        allowed_id in "[a-f0-9]{64}",
        blocked_id in "[a-f0-9]{64}"
    ) {
        prop_assume!(allowed_id != blocked_id);

        let mut rules = VisibilityRules::new();
        let mut allowed = HashSet::new();
        allowed.insert(allowed_id.clone());
        rules.set_contacts(&field_id, allowed);

        prop_assert!(rules.can_see(&field_id, &allowed_id));
        prop_assert!(!rules.can_see(&field_id, &blocked_id));
    }

    /// Property: Default visibility is Everyone
    #[test]
    fn prop_default_is_everyone(
        field_id in field_label_strategy(),
        contact_id in "[a-f0-9]{64}"
    ) {
        let rules = VisibilityRules::new();

        prop_assert!(rules.can_see(&field_id, &contact_id));
        prop_assert_eq!(rules.get(&field_id).clone(), FieldVisibility::Everyone);
    }
}

// ============================================================
// Extended Property Tests (Slow - run with --ignored)
// ============================================================

mod extended_property_tests {
    use super::*;
    use webbook_core::crypto::ratchet::DoubleRatchetState;
    use webbook_core::exchange::X3DHKeyPair;
    use webbook_core::sync::CardDelta;

    /// Strategy for generating message counts for stress tests.
    fn message_count_strategy() -> impl Strategy<Value = usize> {
        100usize..500usize
    }

    /// Strategy for generating delta counts.
    fn delta_count_strategy() -> impl Strategy<Value = usize> {
        20usize..50usize
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10))]

        /// Property: Ratchet handles many consecutive messages correctly.
        /// This is a slow test - run with: cargo test -- --ignored
        #[test]
        #[ignore]
        fn prop_ratchet_many_messages_roundtrip(
            seed in bytes32_strategy(),
            message_count in message_count_strategy()
        ) {
            let shared_secret = SymmetricKey::from_bytes(seed);
            let bob_dh = X3DHKeyPair::generate();

            let mut alice_ratchet =
                DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
            let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

            for i in 0..message_count {
                let plaintext = format!("Message number {}", i);
                let encrypted = alice_ratchet.encrypt(plaintext.as_bytes()).unwrap();
                let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
                prop_assert_eq!(decrypted, plaintext.as_bytes());
            }
        }

        /// Property: Many deltas applied in sequence produce consistent result.
        /// This is a slow test - run with: cargo test -- --ignored
        #[test]
        #[ignore]
        fn prop_sync_many_deltas_converge(
            name in display_name_strategy(),
            delta_count in delta_count_strategy()
        ) {
            let mut card = ContactCard::new(&name);

            for i in 0..delta_count {
                let old_card = card.clone();

                // Add a field
                let field = ContactField::new(
                    FieldType::Custom,
                    &format!("field_{}", i),
                    &format!("value_{}", i),
                );

                // Only add if under limit
                if card.fields().len() < 25 {
                    let _ = card.add_field(field);
                }

                let delta = CardDelta::compute(&old_card, &card);

                // Apply delta to a fresh copy
                let mut verification_card = old_card.clone();
                if !delta.is_empty() {
                    delta.apply(&mut verification_card).unwrap();
                }

                // Cards should match after applying delta
                prop_assert_eq!(card.display_name(), verification_card.display_name());
                prop_assert_eq!(card.fields().len(), verification_card.fields().len());
            }
        }

        /// Property: Field values at maximum length are handled correctly.
        #[test]
        fn prop_field_value_max_length(
            name in display_name_strategy(),
            label in field_label_strategy()
        ) {
            let mut card = ContactCard::new(&name);

            // Create a field with a very long value (1000 chars)
            let long_value: String = (0..1000).map(|i| char::from(b'A' + (i % 26) as u8)).collect();
            let field = ContactField::new(FieldType::Custom, &label, &long_value);

            let result = card.add_field(field);
            prop_assert!(result.is_ok());

            // Verify roundtrip
            let json = serde_json::to_string(&card).unwrap();
            let restored: ContactCard = serde_json::from_str(&json).unwrap();

            let restored_field = restored.fields().iter().find(|f| f.label() == label);
            prop_assert!(restored_field.is_some());
            prop_assert_eq!(restored_field.unwrap().value(), long_value);
        }

        /// Property: Cards with maximum fields are handled correctly.
        #[test]
        fn prop_card_max_fields(
            name in display_name_strategy()
        ) {
            let mut card = ContactCard::new(&name);

            // Add maximum number of fields (25)
            for i in 0..25 {
                let field = ContactField::new(
                    FieldType::Custom,
                    &format!("field_{}", i),
                    &format!("value_{}", i),
                );
                let result = card.add_field(field);
                prop_assert!(result.is_ok(), "Should allow field {} of 25", i + 1);
            }

            prop_assert_eq!(card.fields().len(), 25);

            // 26th field should fail
            let extra_field = ContactField::new(FieldType::Custom, "extra", "value");
            let result = card.add_field(extra_field);
            prop_assert!(result.is_err(), "Should reject field 26");
        }

        /// Property: Bidirectional ratchet conversation maintains correctness.
        #[test]
        fn prop_ratchet_bidirectional(
            seed in bytes32_strategy(),
            exchanges in 1usize..20usize
        ) {
            let shared_secret = SymmetricKey::from_bytes(seed);
            let bob_dh = X3DHKeyPair::generate();

            let mut alice_ratchet =
                DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
            let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

            for i in 0..exchanges {
                // Alice -> Bob
                let alice_msg = format!("Alice message {}", i);
                let enc_a = alice_ratchet.encrypt(alice_msg.as_bytes()).unwrap();
                let dec_a = bob_ratchet.decrypt(&enc_a).unwrap();
                prop_assert_eq!(dec_a, alice_msg.as_bytes());

                // Bob -> Alice
                let bob_msg = format!("Bob message {}", i);
                let enc_b = bob_ratchet.encrypt(bob_msg.as_bytes()).unwrap();
                let dec_b = alice_ratchet.decrypt(&enc_b).unwrap();
                prop_assert_eq!(dec_b, bob_msg.as_bytes());
            }
        }

        /// Property: Version vector with many devices maintains consistency.
        #[test]
        fn prop_version_vector_many_devices(
            device_count in 5usize..20usize,
            ops_per_device in 1usize..10usize
        ) {
            let mut vv = VersionVector::new();
            let mut expected: std::collections::HashMap<[u8; 32], u64> = std::collections::HashMap::new();

            for d in 0..device_count {
                let mut device_id = [0u8; 32];
                device_id[0] = d as u8;

                for _ in 0..ops_per_device {
                    vv.increment(&device_id);
                    *expected.entry(device_id).or_insert(0) += 1;
                }
            }

            // Verify all devices have correct counts
            for (device_id, expected_count) in expected {
                prop_assert_eq!(vv.get(&device_id), expected_count);
            }
        }

        /// Property: Delta computation is deterministic.
        #[test]
        fn prop_delta_deterministic(
            name in display_name_strategy(),
            label in field_label_strategy(),
            value1 in "[a-zA-Z0-9]{1,20}",
            value2 in "[a-zA-Z0-9]{1,20}"
        ) {
            let mut card1 = ContactCard::new(&name);
            card1.add_field(ContactField::new(FieldType::Custom, &label, &value1)).unwrap();

            let mut card2 = ContactCard::new(&name);
            card2.add_field(ContactField::new(FieldType::Custom, &label, &value2)).unwrap();

            // Compute delta twice
            let delta1 = CardDelta::compute(&card1, &card2);
            let delta2 = CardDelta::compute(&card1, &card2);

            // Should be identical
            prop_assert_eq!(delta1.changes.len(), delta2.changes.len());
        }
    }
}

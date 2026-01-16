//! Fuzz Tests for Parsers and Serialization
//!
//! Uses proptest to generate random inputs and verify:
//! 1. Random bytes don't cause panics when deserializing
//! 2. Round-trip serialization works for randomly generated valid structures
//! 3. Malformed inputs are rejected gracefully

use proptest::prelude::*;

// =============================================================================
// ARBITRARY GENERATORS
// =============================================================================

/// Generate arbitrary bytes of given length
fn arbitrary_bytes(len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), len)
}

/// Generate arbitrary JSON-like strings (may be invalid)
fn arbitrary_json_string() -> impl Strategy<Value = String> {
    prop::string::string_regex(r#"[\x00-\x7F]{0,1000}"#)
        .unwrap()
}

// =============================================================================
// CONTACT CARD FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing ContactCard
    #[test]
    fn fuzz_contact_card_deserialize_no_panic(data in arbitrary_json_string()) {
        // This should either succeed or return an error, never panic
        let _ = serde_json::from_str::<webbook_core::ContactCard>(&data);
    }

    /// Valid ContactCard should round-trip correctly
    #[test]
    fn fuzz_contact_card_roundtrip(
        name in "[A-Za-z ]{1,50}",
        email in "[a-z0-9._%+-]+@[a-z0-9.-]+\\.[a-z]{2,4}",
        phone in "[0-9+\\-() ]{5,20}"
    ) {
        let mut card = webbook_core::ContactCard::new(&name);

        // Add fields that might be added
        if !email.is_empty() {
            let _ = card.add_field(webbook_core::ContactField::new(
                webbook_core::FieldType::Email,
                "Work",
                &email,
            ));
        }

        if !phone.is_empty() {
            let _ = card.add_field(webbook_core::ContactField::new(
                webbook_core::FieldType::Phone,
                "Mobile",
                &phone,
            ));
        }

        // Serialize and deserialize
        let serialized = serde_json::to_string(&card).unwrap();
        let deserialized: webbook_core::ContactCard = serde_json::from_str(&serialized).unwrap();

        // Verify
        prop_assert_eq!(card.display_name(), deserialized.display_name());
        prop_assert_eq!(card.fields().len(), deserialized.fields().len());
    }
}

// =============================================================================
// CONTACT FIELD FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing ContactField
    #[test]
    fn fuzz_contact_field_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::ContactField>(&data);
    }

    /// Random bytes should not cause panics when deserializing FieldType
    #[test]
    fn fuzz_field_type_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::FieldType>(&data);
    }
}

// =============================================================================
// SYNC ITEM FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing SyncItem
    #[test]
    fn fuzz_sync_item_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::sync::SyncItem>(&data);
    }

    /// Random bytes should not cause panics when deserializing FieldChange
    #[test]
    fn fuzz_field_change_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::sync::FieldChange>(&data);
    }
}

// =============================================================================
// RATCHET MESSAGE FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing RatchetMessage
    #[test]
    fn fuzz_ratchet_message_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::crypto::ratchet::RatchetMessage>(&data);
    }

    /// Valid RatchetMessage should round-trip correctly
    #[test]
    fn fuzz_ratchet_message_roundtrip(
        dh_public in arbitrary_bytes(32),
        dh_generation in 0u32..1000,
        message_index in 0u32..10000,
        previous_chain_length in 0u32..10000,
        ciphertext in arbitrary_bytes(100)
    ) {
        let dh_public_arr: [u8; 32] = dh_public.try_into().unwrap();

        let msg = webbook_core::crypto::ratchet::RatchetMessage {
            dh_public: dh_public_arr,
            dh_generation,
            message_index,
            previous_chain_length,
            ciphertext: ciphertext.clone(),
        };

        let serialized = serde_json::to_string(&msg).unwrap();
        let deserialized: webbook_core::crypto::ratchet::RatchetMessage =
            serde_json::from_str(&serialized).unwrap();

        prop_assert_eq!(msg.dh_public, deserialized.dh_public);
        prop_assert_eq!(msg.dh_generation, deserialized.dh_generation);
        prop_assert_eq!(msg.message_index, deserialized.message_index);
        prop_assert_eq!(msg.ciphertext, deserialized.ciphertext);
    }
}

// =============================================================================
// VISIBILITY RULES FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing FieldVisibility
    #[test]
    fn fuzz_field_visibility_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::FieldVisibility>(&data);
    }

    /// Random bytes should not cause panics when deserializing VisibilityRules
    #[test]
    fn fuzz_visibility_rules_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::VisibilityRules>(&data);
    }

    /// Valid VisibilityRules should round-trip correctly
    #[test]
    fn fuzz_visibility_rules_roundtrip(
        field_names in prop::collection::hash_set("[a-z]{1,20}", 1..10)
    ) {
        use std::collections::HashMap;

        let mut rules = webbook_core::VisibilityRules::new();
        let mut expected_visibility: HashMap<String, bool> = HashMap::new();

        for (i, name) in field_names.iter().enumerate() {
            if i % 2 == 0 {
                rules.set_everyone(name);
                expected_visibility.insert(name.clone(), true);
            } else {
                rules.set_nobody(name);
                expected_visibility.insert(name.clone(), false);
            }
        }

        let serialized = serde_json::to_string(&rules).unwrap();
        let deserialized: webbook_core::VisibilityRules =
            serde_json::from_str(&serialized).unwrap();

        // Verify each field (using HashSet ensures unique names)
        for (name, expected) in expected_visibility.iter() {
            let actual_visible = deserialized.can_see(name, "any-contact-id");
            prop_assert_eq!(*expected, actual_visible,
                "Visibility mismatch for field {}", name);
        }
    }
}

// =============================================================================
// DEVICE REGISTRY FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing RegisteredDevice
    #[test]
    fn fuzz_registered_device_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::identity::RegisteredDevice>(&data);
    }
}

// =============================================================================
// SOCIAL NETWORK FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Random bytes should not cause panics when deserializing SocialNetwork
    #[test]
    fn fuzz_social_network_deserialize_no_panic(data in arbitrary_json_string()) {
        let _ = serde_json::from_str::<webbook_core::SocialNetwork>(&data);
    }

    /// Valid SocialNetwork should round-trip correctly
    #[test]
    fn fuzz_social_network_roundtrip(
        id in "[a-z]{1,20}",
        display_name in "[A-Za-z ]{1,50}",
        url_template in "https://[a-z]+\\.com/\\{username\\}"
    ) {
        let network = webbook_core::SocialNetwork::new(&id, &display_name, &url_template);

        let serialized = serde_json::to_string(&network).unwrap();
        let deserialized: webbook_core::SocialNetwork =
            serde_json::from_str(&serialized).unwrap();

        prop_assert_eq!(network.id(), deserialized.id());
        prop_assert_eq!(network.display_name(), deserialized.display_name());
    }
}

// =============================================================================
// ENCRYPTION FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Encryption should handle arbitrary plaintext without panicking
    #[test]
    fn fuzz_encryption_arbitrary_plaintext(data in arbitrary_bytes(1000)) {
        use webbook_core::crypto::{encrypt, decrypt, SymmetricKey};

        let key = SymmetricKey::generate();

        // Encrypt should succeed for any input
        let ciphertext = encrypt(&key, &data);
        prop_assert!(ciphertext.is_ok());

        // Decrypt should succeed and return original
        let plaintext = decrypt(&key, &ciphertext.unwrap());
        prop_assert!(plaintext.is_ok());
        prop_assert_eq!(data, plaintext.unwrap());
    }

    /// Decryption of garbage should fail gracefully, not panic
    #[test]
    fn fuzz_decryption_garbage(data in arbitrary_bytes(100)) {
        use webbook_core::crypto::{decrypt, SymmetricKey};

        let key = SymmetricKey::generate();

        // Decrypting garbage should return an error, not panic
        let result = decrypt(&key, &data);
        // Either it fails (expected) or succeeds (unlikely but possible)
        // The important thing is it doesn't panic
        let _ = result;
    }

    /// Wrong key should fail decryption gracefully
    #[test]
    fn fuzz_decryption_wrong_key(data in arbitrary_bytes(100)) {
        use webbook_core::crypto::{encrypt, decrypt, SymmetricKey};

        let key1 = SymmetricKey::generate();
        let key2 = SymmetricKey::generate();

        let ciphertext = encrypt(&key1, &data).unwrap();

        // Decrypting with wrong key should fail
        let result = decrypt(&key2, &ciphertext);
        prop_assert!(result.is_err());
    }
}

// =============================================================================
// HKDF FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// HKDF should handle arbitrary input key material
    #[test]
    fn fuzz_hkdf_arbitrary_ikm(ikm in arbitrary_bytes(100)) {
        use webbook_core::crypto::HKDF;

        let salt = [0u8; 32];
        let info = b"test-info";

        // Should not panic regardless of input
        let result = HKDF::derive(Some(&salt), &ikm, info, 32);
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap().len(), 32);
    }

    /// HKDF should handle various output lengths
    #[test]
    fn fuzz_hkdf_output_lengths(len in 1usize..255) {
        use webbook_core::crypto::HKDF;

        let ikm = [0x42u8; 32];
        let salt = [0u8; 32];
        let info = b"test-info";

        let result = HKDF::derive(Some(&salt), &ikm, info, len);
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap().len(), len);
    }
}

// =============================================================================
// KEY GENERATION FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Key generation should always produce valid keys
    #[test]
    fn fuzz_key_generation_consistency(_seed in any::<u64>()) {
        use webbook_core::crypto::{SigningKeyPair, SymmetricKey};
        use webbook_core::exchange::X3DHKeyPair;

        // Generate various keys - should never panic
        let _sym = SymmetricKey::generate();
        let _signing = SigningKeyPair::generate();
        let _x3dh = X3DHKeyPair::generate();
    }
}

// =============================================================================
// SIGNING FUZZ TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Signing should handle arbitrary messages
    #[test]
    fn fuzz_signing_arbitrary_message(message in arbitrary_bytes(1000)) {
        use webbook_core::crypto::SigningKeyPair;

        let keypair = SigningKeyPair::generate();

        // Sign should work for any input
        let signature = keypair.sign(&message);

        // Verify should succeed for valid signature
        let is_valid = keypair.public_key().verify(&message, &signature);
        prop_assert!(is_valid);
    }

    /// Verification with wrong message should fail
    #[test]
    fn fuzz_signing_wrong_message(
        message1 in arbitrary_bytes(100),
        message2 in arbitrary_bytes(100)
    ) {
        use webbook_core::crypto::SigningKeyPair;

        // Skip if messages happen to be equal
        prop_assume!(message1 != message2);

        let keypair = SigningKeyPair::generate();
        let signature = keypair.sign(&message1);

        // Verifying with different message should fail
        let is_valid = keypair.public_key().verify(&message2, &signature);
        prop_assert!(!is_valid);
    }

    /// Corrupted signature should fail verification
    #[test]
    fn fuzz_signing_corrupted_signature(
        message in arbitrary_bytes(100),
        corruption_index in 0usize..64,
        corruption_byte in any::<u8>()
    ) {
        use webbook_core::crypto::{SigningKeyPair, Signature};

        let keypair = SigningKeyPair::generate();
        let signature = keypair.sign(&message);

        // Corrupt the signature by copying to mutable bytes
        let mut sig_bytes = *signature.as_bytes();
        let original_byte = sig_bytes[corruption_index];
        prop_assume!(corruption_byte != original_byte); // Make sure we actually change it
        sig_bytes[corruption_index] = corruption_byte;

        let corrupted_signature = Signature::from_bytes(sig_bytes);

        // Verification should fail
        let is_valid = keypair.public_key().verify(&message, &corrupted_signature);
        prop_assert!(!is_valid);
    }
}

//! Tests for exchange::encrypted_message
//! Extracted from encrypted_message.rs

use webbook_core::*;
use webbook_core::exchange::*;

    #[test]
    fn test_encrypted_message_basic_roundtrip() {
        let alice = X3DHKeyPair::generate();
        let bob = X3DHKeyPair::generate();

        let alice_identity_key = [0x41u8; 32];
        let alice_name = "Alice";

        let (msg, _) = EncryptedExchangeMessage::create(
            &alice,
            bob.public_key(),
            &alice_identity_key,
            alice_name,
        )
        .unwrap();

        let (payload, _shared_secret) = msg.decrypt(&bob).unwrap();

        assert_eq!(payload.identity_key, alice_identity_key);
        assert_eq!(payload.exchange_key, *alice.public_key());
        assert_eq!(payload.display_name, alice_name);
    }

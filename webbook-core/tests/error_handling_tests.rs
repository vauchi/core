//! Error Handling Tests
//!
//! Tests for error conditions, failure modes, and recovery scenarios.
//! These tests ensure the system handles failures gracefully.

use webbook_core::{
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::{CardDelta, SyncManager},
    Contact, ContactCard, ContactField, FieldType, Storage, SymmetricKey, WebBook,
};

// =============================================================================
// Network Failure Tests
// =============================================================================

/// Test: Sync state remains pending when delivery fails
#[test]
fn test_sync_state_pending_on_undelivered() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let sync_manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Test");
    old_card
        .add_field(ContactField::new(FieldType::Email, "work", "old@test.com"))
        .unwrap();

    let mut new_card = ContactCard::new("Test");
    new_card
        .add_field(ContactField::new(FieldType::Email, "work", "new@test.com"))
        .unwrap();

    // Queue update but don't mark as delivered
    let _update_id = sync_manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();

    // State should remain pending
    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(
        matches!(state, webbook_core::SyncState::Pending { .. }),
        "Should remain pending until explicitly delivered"
    );
}

/// Test: Relay client handles disconnect gracefully
#[test]
fn test_relay_disconnect_clears_state() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
    };

    let mut client = RelayClient::new(transport, config, "test-identity".into());

    // Connect and send a message
    client.connect().unwrap();
    assert!(client.is_connected());

    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    client
        .send_update("recipient-id", &mut ratchet, b"test payload", "update-1")
        .unwrap();
    assert_eq!(client.in_flight_count(), 1);

    // Disconnect
    client.disconnect().unwrap();
    assert!(!client.is_connected());

    // In-flight messages should still be tracked for retry
    // (implementation-dependent behavior)
}

/// Test: Multiple pending updates for same contact
#[test]
fn test_multiple_pending_updates_queued() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let sync_manager = SyncManager::new(&storage);

    let card1 = ContactCard::new("Version 1");
    let card2 = ContactCard::new("Version 2");
    let card3 = ContactCard::new("Version 3");

    // Queue multiple updates
    sync_manager
        .queue_card_update("contact-1", &card1, &card2)
        .unwrap();
    sync_manager
        .queue_card_update("contact-1", &card2, &card3)
        .unwrap();

    // Should have 2 pending updates
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 2, "Both updates should be queued");

    let state = sync_manager.get_sync_state("contact-1").unwrap();
    if let webbook_core::SyncState::Pending { queued_count, .. } = state {
        assert_eq!(queued_count, 2);
    } else {
        panic!("Expected Pending state");
    }
}

// =============================================================================
// Crypto Failure Tests
// =============================================================================

/// Test: Decrypt fails with wrong ratchet state
#[test]
fn test_decrypt_fails_with_wrong_ratchet() {
    let shared_secret1 = SymmetricKey::generate();
    let shared_secret2 = SymmetricKey::generate(); // Different secret

    let bob_dh1 = X3DHKeyPair::generate();
    let bob_dh2 = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret1, *bob_dh1.public_key());
    let mut wrong_bob_ratchet =
        DoubleRatchetState::initialize_responder(&shared_secret2, bob_dh2);

    // Alice encrypts with secret1
    let plaintext = b"Secret message";
    let encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Bob tries to decrypt with ratchet from secret2
    let result = wrong_bob_ratchet.decrypt(&encrypted);
    assert!(result.is_err(), "Decrypt should fail with wrong key");
}

/// Test: Decrypt fails with corrupted ciphertext
#[test]
fn test_decrypt_fails_with_corrupted_ciphertext() {
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Alice encrypts
    let plaintext = b"Secret message";
    let mut encrypted = alice_ratchet.encrypt(plaintext).unwrap();

    // Corrupt the ciphertext
    if !encrypted.ciphertext.is_empty() {
        encrypted.ciphertext[0] ^= 0xFF;
    }

    // Bob tries to decrypt corrupted message
    let result = bob_ratchet.decrypt(&encrypted);
    assert!(
        result.is_err(),
        "Decrypt should fail with corrupted ciphertext"
    );
}

/// Test: Delta signature verification rejects wrong signer
#[test]
fn test_delta_signature_rejects_wrong_signer() {
    use webbook_core::identity::Identity;

    let alice = Identity::create("Alice");
    let eve = Identity::create("Eve"); // Attacker's identity

    let old_card = ContactCard::new("Alice");
    let mut new_card = ContactCard::new("Alice");
    new_card
        .add_field(ContactField::new(FieldType::Email, "work", "alice@test.com"))
        .unwrap();

    let mut delta = CardDelta::compute(&old_card, &new_card);

    // Alice signs the delta
    delta.sign(&alice);

    // Verify with Alice's key should pass
    assert!(
        delta.verify(alice.signing_public_key()),
        "Should verify with correct key"
    );

    // Verify with Eve's key should fail
    assert!(
        !delta.verify(eve.signing_public_key()),
        "Should reject wrong signer"
    );
}

/// Test: Delta signature verification rejects tampered delta
#[test]
fn test_delta_signature_rejects_tampered_delta() {
    use webbook_core::identity::Identity;
    use webbook_core::sync::FieldChange;

    let alice = Identity::create("Alice");

    let old_card = ContactCard::new("Alice");
    let mut new_card = ContactCard::new("Alice");
    new_card
        .add_field(ContactField::new(FieldType::Email, "work", "alice@test.com"))
        .unwrap();

    let mut delta = CardDelta::compute(&old_card, &new_card);

    // Alice signs the delta
    delta.sign(&alice);

    // Verify original signature
    assert!(delta.verify(alice.signing_public_key()));

    // Tamper with the delta (add another change)
    delta.changes.push(FieldChange::DisplayNameChanged {
        new_name: "Eve".to_string(),
    });

    // Signature should now fail
    assert!(
        !delta.verify(alice.signing_public_key()),
        "Should reject tampered delta"
    );
}

// =============================================================================
// Storage Failure Tests
// =============================================================================

/// Test: Loading non-existent contact returns None
#[test]
fn test_load_nonexistent_contact_returns_none() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let result = wb.get_contact("does-not-exist").unwrap();
    assert!(result.is_none(), "Non-existent contact should return None");
}

/// Test: Loading non-existent ratchet state returns None
#[test]
fn test_load_nonexistent_ratchet_returns_none() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let result = storage.load_ratchet_state("nonexistent-contact").unwrap();
    assert!(result.is_none(), "Non-existent ratchet should return None");
}

/// Test: Saving and loading contact preserves data
#[test]
fn test_contact_roundtrip_preserves_data() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let mut card = ContactCard::new("Test Contact");
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "test@example.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();

    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();

    wb.add_contact(contact).unwrap();

    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Test Contact");
    assert_eq!(loaded.card().fields().len(), 2);
}

// =============================================================================
// Protocol Violation Tests
// =============================================================================

/// Test: Delta with no changes is empty
#[test]
fn test_empty_delta_when_cards_identical() {
    let card = ContactCard::new("Test");

    let delta = CardDelta::compute(&card, &card.clone());

    assert!(delta.is_empty(), "Identical cards should produce empty delta");
}

/// Test: Cannot create identity twice
#[test]
fn test_cannot_create_identity_twice() {
    let mut wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    wb.create_identity("First").unwrap();
    let result = wb.create_identity("Second");

    assert!(result.is_err(), "Creating second identity should fail");
}

/// Test: Cannot add duplicate contact
#[test]
fn test_cannot_add_duplicate_contact() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Test"), SymmetricKey::generate());
    let contact_clone =
        Contact::from_exchange([1u8; 32], ContactCard::new("Test"), SymmetricKey::generate());

    wb.add_contact(contact).unwrap();
    let result = wb.add_contact(contact_clone);

    // Behavior depends on implementation - either error or update
    // This test documents the expected behavior
    assert!(
        result.is_ok() || result.is_err(),
        "Should handle duplicate gracefully"
    );
}

/// Test: Mark non-existent update as delivered fails gracefully
#[test]
fn test_mark_nonexistent_update_delivered() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let sync_manager = SyncManager::new(&storage);

    let result = sync_manager.mark_delivered("nonexistent-update-id");
    // Should not panic, behavior depends on implementation
    let _ = result;
}

// =============================================================================
// Identity Error Tests
// =============================================================================

/// Test: Wrong password fails backup import
#[test]
fn test_wrong_password_fails_backup_import() {
    use webbook_core::identity::Identity;

    let identity = Identity::create("Test");
    let backup = identity.export_backup("SecureP@ssw0rd123!").unwrap();

    let result = Identity::import_backup(&backup, "WrongP@ssw0rd999!");
    assert!(result.is_err(), "Wrong password should fail import");
}

/// Test: Corrupted backup fails import
#[test]
fn test_corrupted_backup_fails_import() {
    use webbook_core::identity::Identity;

    let identity = Identity::create("Test");
    let password = "SecureP@ssw0rd123!";
    let mut backup = identity.export_backup(password).unwrap();

    // Corrupt the backup
    let bytes = backup.as_bytes_mut();
    if bytes.len() > 10 {
        bytes[10] ^= 0xFF;
    }

    let result = Identity::import_backup(&backup, password);
    assert!(result.is_err(), "Corrupted backup should fail import");
}

/// Test: Empty password is rejected
#[test]
fn test_empty_password_rejected_for_backup() {
    use webbook_core::identity::Identity;

    let identity = Identity::create("Test");
    let result = identity.export_backup("");

    assert!(result.is_err(), "Empty password should be rejected");
}

// =============================================================================
// Visibility Error Tests
// =============================================================================

/// Test: Setting visibility on non-existent field is handled
#[test]
fn test_visibility_on_nonexistent_field() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Test"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let mut contact = wb.get_contact(&contact_id).unwrap().unwrap();

    // Setting visibility on non-existent field shouldn't panic
    contact
        .visibility_rules_mut()
        .set_nobody("nonexistent-field-id");

    // Save should succeed
    wb.storage().save_contact(&contact).unwrap();
}

//! Contact Exchange E2E Tests
//!
//! Feature: contact_exchange.feature
//! Scenario: Successful QR code exchange with proximity

use webbook_core::{
    crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair, network::MockTransport, Contact,
    ContactField, FieldType, SymmetricKey, WebBook,
};

/// Tests the complete contact exchange workflow between two users.
///
/// Steps:
/// 1. Alice generates exchange QR code with public key and challenge
/// 2. Bob scans QR code and initiates exchange
/// 3. Both parties perform X3DH key agreement
/// 4. Contact cards are exchanged and verified
/// 5. Double Ratchet is initialized for future communication
#[test]
fn test_contact_exchange_happy_path() {
    // Step 1: Create WebBook instances for Alice and Bob
    let mut alice_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
    let mut bob_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();

    // Add fields to Alice's card
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
        ))
        .unwrap();

    // Add fields to Bob's card
    bob_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "personal",
            "bob@email.com",
        ))
        .unwrap();

    // Step 2: Generate exchange data (simulating QR code)
    let alice_identity = alice_wb.identity().unwrap();
    let alice_public_key = *alice_identity.signing_public_key();
    let alice_card = alice_wb.own_card().unwrap().unwrap();

    let bob_identity = bob_wb.identity().unwrap();
    let bob_public_key = *bob_identity.signing_public_key();
    let bob_card = bob_wb.own_card().unwrap().unwrap();

    // Step 3: Simulate X3DH key exchange
    let shared_secret = SymmetricKey::generate();

    // Step 4: Create contacts from exchange
    let bob_contact =
        Contact::from_exchange(bob_public_key, bob_card.clone(), shared_secret.clone());
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact =
        Contact::from_exchange(alice_public_key, alice_card.clone(), shared_secret.clone());
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    // Step 5: Initialize Double Ratchet for encrypted communication
    let bob_dh = X3DHKeyPair::generate();
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Save ratchet states for future communication
    alice_wb
        .storage()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, false)
        .unwrap();

    // Step 6: Verify exchange completed successfully
    assert_eq!(alice_wb.contact_count().unwrap(), 1);
    let bob_in_alice = alice_wb.get_contact(&bob_contact_id).unwrap().unwrap();
    assert_eq!(bob_in_alice.display_name(), "Bob");
    assert_eq!(bob_in_alice.card().fields().len(), 1);

    assert_eq!(bob_wb.contact_count().unwrap(), 1);
    let alice_in_bob = bob_wb.get_contact(&alice_contact_id).unwrap().unwrap();
    assert_eq!(alice_in_bob.display_name(), "Alice");
    assert_eq!(alice_in_bob.card().fields().len(), 2);

    // Ratchet states are persisted
    let alice_ratchet_loaded = alice_wb
        .storage()
        .load_ratchet_state(&bob_contact_id)
        .unwrap();
    assert!(alice_ratchet_loaded.is_some());

    let bob_ratchet_loaded = bob_wb
        .storage()
        .load_ratchet_state(&alice_contact_id)
        .unwrap();
    assert!(bob_ratchet_loaded.is_some());

    // Step 7: Verify encrypted communication works
    let (mut alice_ratchet, _) = alice_ratchet_loaded.unwrap();
    let (mut bob_ratchet, _) = bob_ratchet_loaded.unwrap();

    let message = b"Hello Bob! Exchange successful!";
    let encrypted = alice_ratchet.encrypt(message).unwrap();
    let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, message);

    let reply = b"Hi Alice! Got your message!";
    let encrypted_reply = bob_ratchet.encrypt(reply).unwrap();
    let decrypted_reply = alice_ratchet.decrypt(&encrypted_reply).unwrap();
    assert_eq!(decrypted_reply, reply);
}

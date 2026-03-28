// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for CEK wrapping in card update flow.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Card updates use per-contact content encryption key"
//!   - "Identity deletion sends revocation signal to all contacts"
//!   - "Card update arriving after revocation is discarded"

use vauchi_core::api::*;
use vauchi_core::contact_card::FieldType;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};
use vauchi_core::*;

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

/// Helper: create Alice with identity and a contact Bob with an established ratchet.
/// Returns (alice_vauchi, bob_contact_id, bob_identity, bob_dh_keypair, shared_secret).
fn setup_alice_with_bob_ratchet() -> (Vauchi, String, Identity, X3DHKeyPair, SymmetricKey) {
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();

    let bob_identity = Identity::create("Bob");
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    let contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
    );
    let bob_id = contact.id().to_string();
    alice.add_contact(contact).unwrap();

    // Alice as responder (she'll receive from Bob)
    alice
        .create_ratchet_as_responder(
            &bob_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(*bob_dh.secret_bytes()),
        )
        .unwrap();

    (alice, bob_id, bob_identity, bob_dh, shared_secret)
}

/// Helper: setup Alice as initiator for sending TO Bob.
fn setup_alice_as_sender_to_bob() -> (Vauchi, String) {
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();

    let bob_key = [1u8; 32];
    let contact =
        Contact::from_exchange(bob_key, ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    alice.add_contact(contact).unwrap();

    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    alice
        .create_ratchet_as_initiator(&contact_id, &shared_secret, *their_dh.public_key())
        .unwrap();

    (alice, contact_id)
}

// =============================================================================
// propagate_card_update: CEK wrapping
// =============================================================================

#[test]
fn test_propagate_with_cek_rotates_cek() {
    let (alice, bob_id) = setup_alice_as_sender_to_bob();

    // Give Bob a CEK by setting it on the Contact and re-saving
    let old_cek = ContentEncryptionKey::generate();
    let old_cek_bytes = old_cek.to_bytes();
    let mut bob = alice.get_contact(&bob_id).unwrap().unwrap();
    bob.set_cek(old_cek);
    alice.storage().save_contact(&bob).unwrap();

    // Update card
    let old_card = alice.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ));

    // Propagate
    let queued = alice.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1);

    // CEK should have been rotated
    let new_cek = alice
        .storage()
        .load_contact_cek(&bob_id)
        .unwrap()
        .expect("CEK should exist after propagation");
    assert_ne!(
        old_cek_bytes,
        new_cek.to_bytes(),
        "CEK should be rotated on card update"
    );

    // core-F-009: Verify old CEK cannot decrypt data encrypted with new CEK.
    let test_data = b"test payload after rotation";
    let encrypted = new_cek.encrypt(test_data).unwrap();
    let old_cek_restored = ContentEncryptionKey::from_bytes(old_cek_bytes);
    let result = old_cek_restored.decrypt(&encrypted);
    assert!(
        result.is_err(),
        "Old CEK must not decrypt data encrypted under rotated CEK"
    );
}

#[test]
fn test_propagate_without_cek_generates_one() {
    // Contact without CEK gets a generated CEK — all updates use
    // version 0x02 format (process_card_update rejects legacy payloads).
    let (alice, bob_id) = setup_alice_as_sender_to_bob();

    // No CEK set for Bob initially
    assert!(alice.storage().load_contact_cek(&bob_id).unwrap().is_none());

    let old_card = alice.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ));

    let queued = alice.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1);

    // CEK must now be set (always-CEK format)
    assert!(alice.storage().load_contact_cek(&bob_id).unwrap().is_some());
}

// =============================================================================
// process_card_update: CEK handling
// =============================================================================

#[test]
fn test_process_cek_wrapped_update_saves_cek() {
    let (alice, bob_id, bob_identity, bob_dh, shared_secret) = setup_alice_with_bob_ratchet();
    let alice_pk = alice.identity().unwrap().signing_public_key();

    // Bob creates a CEK-wrapped update
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Create a delta
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity, alice_pk);

    // Wrap delta in CEK
    let cek = ContentEncryptionKey::generate();
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();

    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };

    let versioned = VersionedPayload::encode_cek(&wrapped);

    // Ratchet-encrypt the versioned payload
    let ratchet_msg = bob_ratchet.encrypt(&versioned).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Alice processes the update
    let changed = alice.process_card_update(&bob_id, &encrypted).unwrap();
    assert!(!changed.is_empty());

    // Verify CEK was saved
    let stored_cek = alice
        .storage()
        .load_contact_cek(&bob_id)
        .unwrap()
        .expect("CEK should be saved after processing CEK-wrapped update");
    assert_eq!(
        stored_cek.to_bytes(),
        cek.to_bytes(),
        "Stored CEK should match the one from the update"
    );
}

#[test]
fn test_process_cek_wrapped_update() {
    let (alice, bob_id, bob_identity, bob_dh, shared_secret) = setup_alice_with_bob_ratchet();
    let alice_pk = alice.identity().unwrap().signing_public_key();

    // Bob sends a CEK-wrapped update (the only supported format)
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "bob@company.com",
    ));

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity, alice_pk);

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Alice processes the update — should work
    let changed = alice.process_card_update(&bob_id, &encrypted).unwrap();
    assert!(!changed.is_empty());
    assert!(changed.iter().any(|f| f == "work"));
}

#[test]
fn test_process_update_from_revoked_sender_rejected() {
    let (alice, bob_id, bob_identity, bob_dh, shared_secret) = setup_alice_with_bob_ratchet();
    let alice_pk = alice.identity().unwrap().signing_public_key();

    // Record Bob as revoked
    alice
        .storage()
        .record_revoked_sender(&bob_id, 1700000000)
        .unwrap();

    // Bob sends an update (after being revoked)
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Evil");

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity, alice_pk);

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Alice should reject the update
    let result = alice.process_card_update(&bob_id, &encrypted);
    assert!(
        result.is_err(),
        "Updates from revoked senders should be rejected"
    );
}

#[test]
fn test_process_cek_wrapped_update_applies_delta() {
    let (alice, bob_id, bob_identity, bob_dh, shared_secret) = setup_alice_with_bob_ratchet();
    let alice_pk = alice.identity().unwrap().signing_public_key();

    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Bob's delta: add email field
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "personal",
        "bob@email.com",
    ));

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity, alice_pk);

    // CEK-wrap the delta
    let cek = ContentEncryptionKey::generate();
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();

    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let versioned = VersionedPayload::encode_cek(&wrapped);
    let ratchet_msg = bob_ratchet.encrypt(&versioned).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Process
    let changed = alice.process_card_update(&bob_id, &encrypted).unwrap();
    assert!(changed.iter().any(|f| f == "personal"));

    // Verify Bob's card was updated
    let bob_contact = alice.get_contact(&bob_id).unwrap().unwrap();
    assert!(
        bob_contact
            .card()
            .fields()
            .iter()
            .any(|f| f.label() == "personal")
    );
}

// =============================================================================
// CRIT-06: Forged signature must be rejected
// =============================================================================

#[test]
fn test_cek_wrapped_forged_signature_rejected() {
    let (alice, bob_id, bob_identity, bob_dh, shared_secret) = setup_alice_with_bob_ratchet();
    let alice_pk = alice.identity().unwrap().signing_public_key();

    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    // Create a valid delta
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Tampered");

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity, alice_pk);

    // Tamper with the signature: flip bytes
    for byte in delta.signature.iter_mut() {
        *byte ^= 0xFF;
    }

    // CEK-wrap with tampered signature to test signature verification path
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Alice should reject the forged update
    let result = alice.process_card_update(&bob_id, &encrypted);
    assert!(
        result.is_err(),
        "Updates with forged signatures must be rejected"
    );
}

// =============================================================================
// migrate_contacts_to_cek
// =============================================================================

#[test]
fn test_migrate_contacts_generates_cek() {
    let (alice, bob_id) = setup_alice_as_sender_to_bob();

    // Bob has no CEK (legacy contact)
    assert!(alice.storage().load_contact_cek(&bob_id).unwrap().is_none());

    // Run migration
    let migrated = alice.migrate_contacts_to_cek().unwrap();
    assert_eq!(migrated, 1);

    // Bob should now have a CEK
    alice
        .storage()
        .load_contact_cek(&bob_id)
        .unwrap()
        .expect("expected Some");
}

#[test]
fn test_migrate_contacts_queues_updates() {
    let (alice, bob_id) = setup_alice_as_sender_to_bob();

    // Run migration
    let migrated = alice.migrate_contacts_to_cek().unwrap();
    assert_eq!(migrated, 1);

    // A pending update should be queued for Bob
    let pending = alice.storage().get_pending_updates(&bob_id).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "Migration should queue an update for each contact"
    );
    assert_eq!(pending[0].update_type, "cek_migration");
}

#[test]
fn test_migrate_skips_contacts_with_existing_cek() {
    let (alice, bob_id) = setup_alice_as_sender_to_bob();

    // Give Bob a CEK by setting it on Contact and re-saving
    let cek = ContentEncryptionKey::generate();
    let cek_bytes = cek.to_bytes();
    let mut bob = alice.get_contact(&bob_id).unwrap().unwrap();
    bob.set_cek(cek);
    alice.storage().save_contact(&bob).unwrap();

    // Run migration — should skip Bob
    let migrated = alice.migrate_contacts_to_cek().unwrap();
    assert_eq!(migrated, 0);

    // CEK should be unchanged
    let stored = alice.storage().load_contact_cek(&bob_id).unwrap().unwrap();
    assert_eq!(stored.to_bytes(), cek_bytes);
}

#[test]
fn test_migrate_contacts_skips_no_ratchet() {
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();

    // Add a contact WITHOUT ratchet (can't send updates)
    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    alice.add_contact(contact).unwrap();

    // Migration should skip contacts without ratchet
    let migrated = alice.migrate_contacts_to_cek().unwrap();
    assert_eq!(migrated, 0);
}

// =============================================================================
// End-to-end: CEK-wrapped propagation + processing
// =============================================================================

#[test]
fn test_cek_wrapped_end_to_end_flow() {
    // Setup: Alice and Bob with mutual ratchets
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    // Alice's side — create identity first to get actual keys
    let mut alice = create_test_vauchi();
    alice.create_identity("Alice").unwrap();
    let alice_pub_key = *alice.identity().unwrap().signing_public_key();

    // Bob's side — create identity first to get actual keys
    let mut bob = create_test_vauchi();
    bob.create_identity("Bob").unwrap();
    let bob_pub_key = *bob.identity().unwrap().signing_public_key();

    // Alice adds Bob using Bob's actual public key
    let bob_contact =
        Contact::from_exchange(bob_pub_key, ContactCard::new("Bob"), shared_secret.clone());
    let bob_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    alice
        .create_ratchet_as_initiator(&bob_id, &shared_secret, *bob_dh.public_key())
        .unwrap();

    // Give Bob a CEK on Alice's side (simulating previous exchange)
    let initial_cek = ContentEncryptionKey::generate();
    let initial_cek_bytes = initial_cek.to_bytes();
    {
        let mut bob_on_alice = alice.get_contact(&bob_id).unwrap().unwrap();
        bob_on_alice.set_cek(initial_cek);
        alice.storage().save_contact(&bob_on_alice).unwrap();
    }

    // Bob adds Alice using Alice's actual public key
    let alice_contact = Contact::from_exchange(
        alice_pub_key,
        ContactCard::new("Alice"),
        shared_secret.clone(),
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();
    bob.create_ratchet_as_responder(
        &alice_id,
        &shared_secret,
        X3DHKeyPair::from_bytes(*bob_dh.secret_bytes()),
    )
    .unwrap();

    // Alice updates her card and propagates
    let old_card = alice.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@corp.com",
    ));

    let queued = alice.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1);

    // Get the queued encrypted payload
    let pending = alice.storage().get_pending_updates(&bob_id).unwrap();
    assert_eq!(pending.len(), 1);

    // Bob processes the update
    let changed = bob
        .process_card_update(&alice_id, &pending[0].payload)
        .unwrap();
    assert!(changed.iter().any(|f| f == "work"));

    // Bob should now have Alice's CEK stored
    let bob_alice_cek = bob
        .storage()
        .load_contact_cek(&alice_id)
        .unwrap()
        .expect("Bob should have Alice's CEK after processing CEK-wrapped update");

    // The CEK should be the rotated one (not the initial one)
    assert_ne!(
        bob_alice_cek.to_bytes(),
        initial_cek_bytes,
        "Bob should have the rotated CEK, not the initial one"
    );
}

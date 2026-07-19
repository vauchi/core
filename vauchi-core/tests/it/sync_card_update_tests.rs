// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the shared card update processing pipeline (sync/card_update.rs).
//!
//! Covers: revocation, blocking, ratchet decrypt, versioned payload,
//! signature verification, replay detection, delta apply, atomic txn.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{
    CardUpdateError, process_card_updates, process_single_card_update,
    process_single_card_update_for_device,
};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

/// Helper: create Alice and Bob with a mutual contact, ratchet states stored,
/// and return everything needed to construct and process card updates.
///
/// Returns: (alice_wb, bob_wb, shared_secret, bob_contact_id_at_alice, alice_contact_id_at_bob)
pub(crate) fn setup_exchange_with_ratchets() -> (
    vauchi_core::Vauchi,
    vauchi_core::Vauchi,
    SymmetricKey,
    String,
    String,
) {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();

    let shared_secret = SymmetricKey::generate();

    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        shared_secret.clone(),
        0,
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    // Create ratchets: Bob is initiator so he can encrypt to Alice,
    // Alice is responder so she can decrypt Bob's messages.
    let alice_dh = X3DHKeyPair::generate();
    let bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *alice_dh.public_key()).unwrap();
    let alice_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, alice_dh);

    // Store Alice's ratchet for Bob in Alice's storage
    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, false)
        .unwrap();

    // Store Bob's ratchet for Alice in Bob's storage
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, true)
        .unwrap();

    (
        alice_wb,
        bob_wb,
        shared_secret,
        bob_contact_id,
        alice_contact_id,
    )
}

/// Helper: create a valid card update ciphertext that Bob sends to Alice.
///
/// 1. Compute delta between old and new card
/// 2. Sign with Bob's identity, binding to Alice as recipient
/// 3. CEK-wrap the delta bytes (v0x02)
/// 4. Encrypt with Bob's ratchet
/// 5. Serialize RatchetMessage to JSON
fn create_valid_update(
    bob_wb: &vauchi_core::Vauchi,
    alice_signing_pk: &[u8; 32],
    alice_contact_id: &str,
    old_card: &ContactCard,
    new_card: &ContactCard,
) -> Vec<u8> {
    // Default delta version is 1, so this matches the historical behavior.
    create_valid_update_versioned(
        bob_wb,
        alice_signing_pk,
        alice_contact_id,
        old_card,
        new_card,
        1,
    )
}

/// Like [`create_valid_update`] but stamps an explicit delta version (#42), so a
/// test can drive the receiver's stale-version downgrade rejection.
pub(crate) fn create_valid_update_versioned(
    bob_wb: &vauchi_core::Vauchi,
    alice_signing_pk: &[u8; 32],
    alice_contact_id: &str,
    old_card: &ContactCard,
    new_card: &ContactCard,
    version: u32,
) -> Vec<u8> {
    create_valid_update_versioned_for_device(
        bob_wb,
        alice_signing_pk,
        alice_contact_id,
        &[0; 32],
        old_card,
        new_card,
        version,
    )
}

/// Like [`create_valid_update_versioned`] but seals through the ratchet
/// session of one specific sender device, so a test can drive the
/// device-aware receive path (`process_single_card_update_for_device`).
pub(crate) fn create_valid_update_versioned_for_device(
    bob_wb: &vauchi_core::Vauchi,
    alice_signing_pk: &[u8; 32],
    alice_contact_id: &str,
    bob_device_id: &[u8; 32],
    old_card: &ContactCard,
    new_card: &ContactCard,
    version: u32,
) -> Vec<u8> {
    let bob_identity = bob_wb.identity().unwrap();

    let mut delta = CardDelta::compute(old_card, new_card, 0);
    delta.set_version(version);
    delta.sign(bob_identity, alice_signing_pk);

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

    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state_for_device(alice_contact_id, bob_device_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(alice_contact_id, bob_device_id, &bob_ratchet, is_init)
        .unwrap();

    serde_json::to_vec(&ratchet_msg).unwrap()
}

// --- Tests ---

// @internal
#[test]
fn test_process_empty_batch() {
    let alice_wb = create_vauchi_with_identity("Alice");
    let alice_identity = alice_wb.identity().unwrap();

    let result = process_card_updates(alice_identity, alice_wb.storage(), vec![]).unwrap();

    assert_eq!(result.processed, 0, "No updates processed");
    assert_eq!(result.skipped, 0, "No updates skipped");
}

// @internal
#[test]
fn test_process_single_valid_update() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@test.com",
            0,
        ))
        .unwrap();

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(result.is_ok(), "Valid update should succeed: {:?}", result);

    let contact = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    let has_email = contact
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == "Email" && f.value() == "bob@test.com");
    assert!(has_email, "Card should have the new email field");
}

// @internal
#[test]
fn test_sender_revoked_rejected() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    alice_wb
        .storage()
        .contacts()
        .record_revoked_sender(&bob_contact_id, 1000)
        .unwrap();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(result.is_err(), "Revoked sender should be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("SenderRevoked"),
        "Error should be SenderRevoked, got: {err}"
    );
}

// @internal
#[test]
fn test_contact_not_found_rejected() {
    let alice_wb = create_vauchi_with_identity("Alice");

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        "nonexistent_contact_id",
        b"irrelevant_ciphertext",
    );

    assert!(result.is_err(), "Unknown sender should be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("ContactNotFound"),
        "Error should be ContactNotFound, got: {err}"
    );
}

// @internal
#[test]
fn test_contact_blocked_rejected() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    alice_wb.block_contact(&bob_contact_id).unwrap();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(result.is_err(), "Blocked contact should be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("ContactBlocked"),
        "Error should be ContactBlocked, got: {err}"
    );
}

// @internal
#[test]
fn test_no_ratchet_state_rejected() {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    // No ratchet state stored — just send garbage ciphertext
    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        b"irrelevant",
    );

    assert!(result.is_err(), "Missing ratchet should be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("NoRatchetState"),
        "Error should be NoRatchetState, got: {err}"
    );
}

// @internal
#[test]
fn test_invalid_ratchet_message_rejected() {
    let (alice_wb, _bob_wb, _shared_secret, bob_contact_id, _alice_contact_id) =
        setup_exchange_with_ratchets();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        b"not valid json",
    );

    assert!(
        result.is_err(),
        "Invalid ratchet message should be rejected"
    );
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("InvalidRatchetMessage"),
        "Error should be InvalidRatchetMessage, got: {err}"
    );
}

// @internal
#[test]
fn test_decryption_failed_rejected() {
    let (alice_wb, _bob_wb, _shared_secret, bob_contact_id, _alice_contact_id) =
        setup_exchange_with_ratchets();

    // Create a ratchet message encrypted with a DIFFERENT ratchet (wrong key)
    let different_secret = SymmetricKey::generate();
    let wrong_dh = X3DHKeyPair::generate();
    let mut wrong_ratchet =
        DoubleRatchetState::initialize_initiator(&different_secret, *wrong_dh.public_key())
            .unwrap();
    let ratchet_msg = wrong_ratchet.encrypt(b"some payload").unwrap();
    let ciphertext = serde_json::to_vec(&ratchet_msg).unwrap();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(result.is_err(), "Wrong key decryption should fail");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("DecryptionFailed"),
        "Error should be DecryptionFailed, got: {err}"
    );
}

// @internal
#[test]
fn test_signature_invalid_rejected() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    // Create a delta but sign with a WRONG recipient key (tampered signature binding)
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Work",
            "bob@work.com",
            0,
        ))
        .unwrap();

    let bob_identity = bob_wb.identity().unwrap();
    let mut delta = CardDelta::compute(&old_card, &new_card, 0);

    // Sign with wrong recipient pk (random bytes)
    let wrong_recipient_pk = [0xABu8; 32];
    delta.sign(bob_identity, &wrong_recipient_pk);

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

    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, is_init)
        .unwrap();

    let ciphertext = serde_json::to_vec(&ratchet_msg).unwrap();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(result.is_err(), "Invalid signature should be rejected");
    let err = format!("{:?}", result.unwrap_err());
    assert!(
        err.contains("SignatureInvalid"),
        "Error should be SignatureInvalid, got: {err}"
    );
}

// @internal
#[test]
fn test_replay_detected() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+1234567890",
            0,
        ))
        .unwrap();

    let ciphertext1 = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    let result1 = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext1,
    );
    assert!(
        result1.is_ok(),
        "First update should succeed: {:?}",
        result1
    );

    // Pre-record a nonce so the next update with the same nonce is rejected.
    let bob_identity = bob_wb.identity().unwrap();
    let test_nonce = [42u8; 32];
    alice_wb
        .storage()
        .replay()
        .save_replay_nonce(&bob_contact_id, &test_nonce, 1000)
        .unwrap();

    let mut new_card2 = ContactCard::new("Bob");
    new_card2
        .add_field(ContactField::new(
            FieldType::Email,
            "Email2",
            "bob2@test.com",
            0,
        ))
        .unwrap();

    let mut delta2 = CardDelta::compute(&old_card, &new_card2, 0);
    delta2.nonce = test_nonce;
    delta2.sign(bob_identity, &alice_signing_pk);

    let delta_bytes2 = serde_json::to_vec(&delta2).unwrap();
    let cek2 = ContentEncryptionKey::generate();
    let cek_ciphertext2 = cek2.encrypt(&delta_bytes2).unwrap();
    let wrapped2 = CekWrappedPayload {
        cek: cek2.to_bytes(),
        cek_ciphertext: cek_ciphertext2,
        signature: delta2.signature,
        nonce: delta2.nonce,
    };
    let payload2 = VersionedPayload::encode_cek(&wrapped2);

    let (mut bob_ratchet2, is_init2) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg2 = bob_ratchet2.encrypt(&payload2).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet2, is_init2)
        .unwrap();

    let ciphertext2 = serde_json::to_vec(&ratchet_msg2).unwrap();

    let result2 = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext2,
    );

    assert!(result2.is_err(), "Replay should be detected");
    let err = format!("{:?}", result2.unwrap_err());
    assert!(
        err.contains("ReplayDetected"),
        "Error should be ReplayDetected, got: {err}"
    );
}

// @internal
#[test]
fn test_batch_partial_failure() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@batch.com",
            0,
        ))
        .unwrap();

    let valid_ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    // Invalid update (garbage from unknown sender)
    let updates = vec![
        ("unknown_sender".to_string(), b"garbage".to_vec()),
        (bob_contact_id.clone(), valid_ciphertext),
    ];

    let result =
        process_card_updates(alice_wb.identity().unwrap(), alice_wb.storage(), updates).unwrap();

    assert_eq!(result.skipped, 1, "One invalid update should be skipped");
    assert_eq!(result.processed, 1, "One valid update should be processed");
}

// @internal
#[test]
fn test_decode_versioned_payload_via_helper() {
    // Test via a full pipeline: create a CEK-wrapped update and verify it processes
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(
        result.is_ok(),
        "CEK-wrapped versioned payload should be processed successfully: {:?}",
        result
    );
}

// @internal
#[test]
fn test_decode_versioned_payload_cek_wrapped() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_identity = bob_wb.identity().unwrap();

    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob CEK");

    let mut delta = CardDelta::compute(&old_card, &new_card, 0);
    delta.sign(bob_identity, &alice_signing_pk);

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

    let (mut bob_ratchet, is_init) = bob_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&alice_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&payload).unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, is_init)
        .unwrap();

    let ciphertext = serde_json::to_vec(&ratchet_msg).unwrap();

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );

    assert!(
        result.is_ok(),
        "CEK-wrapped versioned payload should be processed successfully: {:?}",
        result
    );
}

// ============================================================
// Anonymous Sender Resolution Tests (SP-32)
// Traces to: features/anonymous_sender.feature @wire
// Verifies that process_card_updates resolves anonymous sender IDs
// to real contact IDs before storage lookups.
// ============================================================

// @scenario: anonymous_sender :: Incoming messages with anonymous sender ID are resolved
// @internal
#[test]
fn test_process_card_update_resolves_anonymous_sender_id() {
    use vauchi_core::network::anonymous::AnonymousSender;

    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob Updated");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@anon.test",
            0,
        ))
        .unwrap();

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    // Compute Bob's anonymous sender ID using the shared key Alice has for Bob
    let bob_contact = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    // Slice 14 made `now` explicit; mirror the storage clock that
    // `process_card_updates` will use, so both sides compute the
    // same epoch and the anonymous-ID resolution succeeds.
    let now_for_epoch = alice_wb.storage().clock().unix_seconds();
    let anon = AnonymousSender::for_current_epoch(
        bob_contact.shared_key().unwrap().as_bytes(),
        now_for_epoch,
    );
    let anonymous_id_hex = hex::encode(anon.anonymous_id);

    // Process using anonymous sender ID instead of real contact ID
    let result = process_card_updates(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        vec![(anonymous_id_hex.clone(), ciphertext)],
    )
    .unwrap();

    assert_eq!(
        result.processed, 1,
        "Update with anonymous sender ID should be processed (resolved to real contact)"
    );
    assert_eq!(result.skipped, 0, "No updates should be skipped");
}

// @scenario: anonymous_sender :: Unknown anonymous sender ID is handled gracefully
// @internal
#[test]
fn test_process_card_update_skips_unresolvable_anonymous_id() {
    let alice_wb = create_vauchi_with_identity("Alice");

    // Fake ciphertext with unknown anonymous sender ID
    let unknown_anon_hex = hex::encode([0xFFu8; 32]);
    let fake_ciphertext = b"not real ciphertext".to_vec();

    let result = process_card_updates(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        vec![(unknown_anon_hex, fake_ciphertext)],
    )
    .unwrap();

    assert_eq!(
        result.processed, 0,
        "Unknown sender should not be processed"
    );
    assert_eq!(result.skipped, 1, "Unknown sender should be skipped");
}

// ============================================================
// Field Note Cleanup on Inbound FieldChange::Removed (Task 10)
// Traces to: features/contact_field_notes.feature @inbound_field_removed
// ============================================================

// @scenario: contact_field_notes :: per-field note is deleted when contact removes that field
// @scenario: contact_field_notes :: notes on retained fields survive when another field is removed
// @internal
#[test]
fn test_field_note_cleaned_on_inbound_field_removed() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();

    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    // Bob's card starts with TWO fields: Email (will be removed) and Phone (will be kept).
    let mut old_card = ContactCard::new("Bob");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@test.com",
            0,
        ))
        .unwrap();
    old_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+41791234567",
            0,
        ))
        .unwrap();
    let removed_field_id = old_card.fields()[0].id().to_string();
    let retained_field_id = old_card.fields()[1].id().to_string();

    // Persist the initial card into Alice's storage so her contact reflects it.
    let mut alice_bob_contact = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    alice_bob_contact.update_card(old_card.clone(), 0);
    alice_wb
        .storage()
        .contacts()
        .save_contact(&alice_bob_contact)
        .unwrap();

    alice_wb
        .storage()
        .field_notes()
        .save_contact_field_note(
            &bob_contact_id,
            &removed_field_id,
            b"met at conference 2026",
        )
        .unwrap();
    alice_wb
        .storage()
        .field_notes()
        .save_contact_field_note(&bob_contact_id, &retained_field_id, b"best number to call")
        .unwrap();

    let notes_before = alice_wb
        .storage()
        .field_notes()
        .load_contact_field_notes(&bob_contact_id)
        .unwrap();
    assert_eq!(
        notes_before.len(),
        2,
        "Both field notes should exist before update"
    );

    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+41791234567",
            0,
        ))
        .unwrap();
    // Give new_card the same field_id for Phone so the delta only shows Email removed.
    // Since field_id is random, we must copy the retained field from old_card directly.
    let mut new_card = ContactCard::new("Bob");
    for f in old_card.fields() {
        if f.id() == retained_field_id {
            new_card.add_field(f.clone()).unwrap();
        }
    }

    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    let result = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    );
    assert!(
        result.is_ok(),
        "Card update with field removal should succeed: {:?}",
        result
    );

    // CRITICAL: The orphaned note for the removed field must be gone,
    // but the note for the retained field must survive.
    let notes_after = alice_wb
        .storage()
        .field_notes()
        .load_contact_field_notes(&bob_contact_id)
        .unwrap();
    assert!(
        !notes_after.contains_key(&removed_field_id),
        "Field note for removed field '{removed_field_id}' should be deleted after inbound FieldChange::Removed"
    );
    assert!(
        notes_after.contains_key(&retained_field_id),
        "Field note for retained field '{retained_field_id}' must NOT be deleted — only orphaned notes are cleaned up"
    );
}

// A `Removed` must FULLY revoke even when the recipient's STORED card carries a
// pre-upsert duplicate of that field. The shipping device receiver
// (process_single_card_update) must heal duplicates before applying, or one
// stale copy stays visible on the recipient — a privacy leak. Cards stored
// before `add_field` became an upsert (2026-06-14-delta-apply-duplicate-fields)
// accumulated such duplicates; the test-only receiver healed them, the device
// receiver did not (root cause 2026-06-29-card-update-duplicate-message-paths
// finding #1).
// @scenario: sync_updates :: A removed field fully revokes despite a stored duplicate
#[test]
fn removed_field_fully_revokes_despite_stored_duplicate() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    // Bob's authentic card: a single Email field.
    let mut old_card = ContactCard::new("Bob");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@test.com",
            0,
        ))
        .unwrap();
    let email_id = old_card.fields()[0].id().to_string();

    // Persist a CORRUPTED copy into Alice's storage: the Email field appears
    // twice. add_field upserts now, so the duplicate is injected via serde
    // (mirrors deduplicate_fields_collapses_same_id_duplicates).
    let mut value = serde_json::to_value(&old_card).unwrap();
    let fields = value.get_mut("fields").unwrap().as_array_mut().unwrap();
    let dup = fields[0].clone();
    fields.push(dup);
    let corrupted: ContactCard = serde_json::from_value(value).unwrap();
    assert_eq!(
        corrupted.fields().len(),
        2,
        "constructed a duplicated stored card"
    );

    let mut alice_bob = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    alice_bob.update_card(corrupted, 0);
    alice_wb
        .storage()
        .contacts()
        .save_contact(&alice_bob)
        .unwrap();

    // Bob removes the Email field (delta computed from his authentic 1-field card).
    let new_card = ContactCard::new("Bob");
    let ciphertext = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );

    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    )
    .expect("removal update must apply");

    let stored = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    let leaked = stored
        .card()
        .fields()
        .iter()
        .filter(|f| f.id() == email_id)
        .count();
    assert_eq!(
        leaked, 0,
        "a single Removed left {leaked} stale copy(ies) of a revoked field"
    );
}

// The shipping device receiver must reject a stale (downgraded) delta version
// (#42) and record the version it applied. A withheld or reordered older delta
// carries an unseen nonce, so it passes replay detection; without a version
// floor it downgrades the stored card. The test-only receiver enforced this;
// the device receiver neither checked nor recorded the version
// (root cause 2026-06-29-card-update-duplicate-message-paths finding #2).
// @scenario: sync_updates :: A stale-version delta is rejected by the device receiver
#[test]
fn stale_version_delta_rejected_by_device_receiver() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let base = ContactCard::new("Bob");

    // v2: Bob adds a Phone field, stamped delta version 2.
    let mut card_v2 = ContactCard::new("Bob");
    card_v2
        .add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+41 79 000",
            0,
        ))
        .unwrap();
    let ct_v2 = create_valid_update_versioned(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &base,
        &card_v2,
        2,
    );

    // v1: an OLDER delta (version 1) adding a different Email field, encrypted
    // after v2 so the relay could deliver it second — the downgrade window.
    let mut card_v1 = ContactCard::new("Bob");
    card_v1
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "stale@x.com",
            0,
        ))
        .unwrap();
    let ct_v1 = create_valid_update_versioned(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &base,
        &card_v1,
        1,
    );

    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ct_v2,
    )
    .expect("v2 update must apply");
    assert_eq!(
        alice_wb
            .storage()
            .contacts()
            .last_delta_version_for_device(&bob_contact_id, &[0; 32])
            .unwrap(),
        2,
        "applying v2 must record delta version 2"
    );

    let stale = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ct_v1,
    );
    assert!(
        matches!(
            stale,
            Err(CardUpdateError::StaleVersion { delta: 1, last: 2 })
        ),
        "a version-1 delta after version 2 must be rejected as StaleVersion{{delta:1,last:2}}; got {stale:?}"
    );

    // The stored card must still reflect v2 only — no downgrade leaked through.
    let stored = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    assert!(
        stored.card().fields().iter().any(|f| f.label() == "Phone"),
        "v2 Phone field must remain after the stale delta is rejected"
    );
    assert!(
        !stored.card().fields().iter().any(|f| f.label() == "Email"),
        "the stale v1 Email field must NOT have been applied"
    );
    assert_eq!(
        alice_wb
            .storage()
            .contacts()
            .last_delta_version_for_device(&bob_contact_id, &[0; 32])
            .unwrap(),
        2,
        "a rejected stale delta must not move the recorded version"
    );
}

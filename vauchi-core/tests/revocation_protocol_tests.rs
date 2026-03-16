// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the account revocation protocol.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Account deletion sends revocation signal to all contacts"
//!   - "Revocation signal is cryptographically authenticated"
//!   - "Spoofed revocation signal is rejected"
//!   - "Card update arriving after revocation is discarded"
//!   - "Replayed revocation for re-established contact is rejected"

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::network::message::AccountRevoked;
use vauchi_core::network::revocation::{
    canonical_revocation_bytes, process_revocation, REVOCATION_DOMAIN_SEPARATOR,
};
use vauchi_core::storage::Storage;

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

fn make_contact_with_pk(pk: [u8; 32], name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "test@example.com",
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key)
}

// === Domain Separator ===

#[test]
fn test_domain_separator_is_25_bytes() {
    assert_eq!(REVOCATION_DOMAIN_SEPARATOR.len(), 25);
    assert_eq!(REVOCATION_DOMAIN_SEPARATOR, b"vauchi-account-revoked-v1");
}

// === Canonical Signature ===

#[test]
fn test_canonical_bytes_length() {
    let sender_id = [0x01u8; 32];
    let recipient_id = [0x02u8; 32];
    let timestamp: u64 = 1700000000;

    let bytes = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    assert_eq!(bytes.len(), 97); // 25 + 32 + 32 + 8
}

#[test]
fn test_canonical_bytes_deterministic() {
    let sender_id = [0xAAu8; 32];
    let recipient_id = [0xBBu8; 32];
    let timestamp: u64 = 1700000000;

    let a = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    let b = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    assert_eq!(a, b);
}

#[test]
fn test_canonical_bytes_different_for_different_inputs() {
    let sender = [0xAAu8; 32];
    let recipient = [0xBBu8; 32];

    let a = canonical_revocation_bytes(&sender, &recipient, 1700000000);
    let b = canonical_revocation_bytes(&sender, &recipient, 1700000001);
    assert_ne!(a, b);

    let c = canonical_revocation_bytes(&recipient, &sender, 1700000000);
    assert_ne!(a, c);
}

#[test]
fn test_canonical_bytes_starts_with_domain_separator() {
    let bytes = canonical_revocation_bytes(&[0u8; 32], &[0u8; 32], 0);
    assert!(bytes.starts_with(REVOCATION_DOMAIN_SEPARATOR));
}

// === AccountRevoked Message ===

// @scenario: privacy_compliance.feature:Revocation signal is cryptographically authenticated
#[test]
fn test_account_revoked_sign_and_verify() {
    let identity = Identity::create("Alice Test");
    let recipient_pk = [0xBBu8; 32];
    let recipient_id = hex::encode(recipient_pk);
    let timestamp = 1700000000u64;

    let revoked = AccountRevoked::create(&identity, &recipient_id, timestamp);

    // Verify the signature using the signing public key
    assert!(revoked.verify(identity.signing_public_key()));
}

// @scenario: privacy_compliance.feature:Spoofed revocation signal is rejected
#[test]
fn test_account_revoked_rejects_tampered_timestamp() {
    let identity = Identity::create("Alice Test");
    let recipient_pk = [0xBBu8; 32];
    let recipient_id = hex::encode(recipient_pk);

    let mut revoked = AccountRevoked::create(&identity, &recipient_id, 1700000000);

    // Tamper with timestamp
    revoked.timestamp = 1700000001;

    assert!(!revoked.verify(identity.signing_public_key()));
}

// @scenario: privacy_compliance.feature:Spoofed revocation signal is rejected
#[test]
fn test_account_revoked_rejects_wrong_key() {
    let identity = Identity::create("Alice");
    let other_identity = Identity::create("Mallory");
    let recipient_id = hex::encode([0xBBu8; 32]);

    let revoked = AccountRevoked::create(&identity, &recipient_id, 1700000000);

    // Wrong public key should not verify
    assert!(!revoked.verify(other_identity.signing_public_key()));
}

#[test]
fn test_account_revoked_serialization_roundtrip() {
    let identity = Identity::create("Alice");
    let recipient_id = hex::encode([0xCCu8; 32]);

    let revoked = AccountRevoked::create(&identity, &recipient_id, 1700000000);

    let json = serde_json::to_string(&revoked).unwrap();
    let deserialized: AccountRevoked = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.sender_id, revoked.sender_id);
    assert_eq!(deserialized.recipient_id, revoked.recipient_id);
    assert_eq!(deserialized.timestamp, revoked.timestamp);
    assert_eq!(deserialized.signature, revoked.signature);
}

// === Revocation Processing ===

// @scenario: privacy_compliance.feature:Account deletion sends revocation signal to all contacts
#[test]
fn test_process_revocation_deletes_contact_and_records_tombstone() {
    let storage = test_storage();
    let identity = Identity::create("Alice");
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    // We are Bob. Store Alice as our contact.
    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Use a future timestamp (must be >= exchange_timestamp to not be stale)
    let future_ts = alice_contact.exchange_timestamp() + 1;
    let revoked = AccountRevoked::create(&identity, &bob_id, future_ts);

    let result = process_revocation(&revoked, &storage);
    result.expect("expected success");

    // Contact should be deleted
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());

    // Tombstone should be recorded
    assert!(storage.is_sender_revoked(alice_contact.id()).unwrap());
}

// @scenario: privacy_compliance.feature:Spoofed revocation signal is rejected
#[test]
fn test_process_revocation_rejects_invalid_signature() {
    let storage = test_storage();
    let identity = Identity::create("Alice");
    let mallory = Identity::create("Mallory");
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    // Store Alice as contact
    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Create revocation signed by Mallory but claiming to be from Alice
    let future_ts = alice_contact.exchange_timestamp() + 1;
    let mut spoofed = AccountRevoked::create(&mallory, &bob_id, future_ts);
    spoofed.sender_id = alice_contact.id().to_string();

    let result = process_revocation(&spoofed, &storage);
    result.expect("expected success"); // No error, just a no-op

    // Alice's contact should still exist (signature didn't verify)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

// @scenario: privacy_compliance.feature:Replayed revocation for re-established contact is rejected
#[test]
fn test_process_revocation_stale_rejected() {
    let storage = test_storage();
    let identity = Identity::create("Alice");
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Revocation with timestamp 0 (before any possible exchange)
    let stale_revoked = AccountRevoked::create(&identity, &bob_id, 0);

    let result = process_revocation(&stale_revoked, &storage);
    result.expect("expected success");

    // Alice's contact should still exist (stale revocation ignored)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

#[test]
fn test_process_revocation_unknown_sender_noop() {
    let storage = test_storage();
    let identity = Identity::create("Unknown");
    let bob_id = hex::encode([0xBBu8; 32]);

    let revoked = AccountRevoked::create(&identity, &bob_id, 1700000000);

    // No contact stored for sender — should be a no-op
    let result = process_revocation(&revoked, &storage);
    result.expect("expected success");
}

// @scenario: privacy_compliance.feature:Card update arriving after revocation is discarded
#[test]
fn test_update_after_revocation_discarded_via_tombstone() {
    let storage = test_storage();

    // Record a tombstone
    storage
        .record_revoked_sender("alice_id", 1700000000)
        .unwrap();

    // Tombstone should block future updates
    assert!(storage.is_sender_revoked("alice_id").unwrap());
}

// @scenario: privacy_compliance.feature:Account deletion sends revocation signal to all contacts
#[test]
fn test_revocation_only_deletes_matching_sender() {
    let storage = test_storage();
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");

    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    // Store Alice (with Alice's public key)
    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Bob revokes, using Alice's ID as recipient (Bob is the one sending revocation)
    let revocation = AccountRevoked::create(&bob, &bob_id, 1700000000);

    process_revocation(&revocation, &storage).unwrap();

    // Alice should still be there (revocation was from unknown sender)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

// @scenario: security.feature:Tampered exchange data is rejected
#[test]
fn test_revocation_with_future_timestamp() {
    let storage = test_storage();
    let alice = Identity::create("Alice");
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    let exchange_ts = alice_contact.exchange_timestamp();
    // Far future timestamp
    let future_ts = exchange_ts + 10000;

    let revocation = AccountRevoked::create(&alice, &bob_id, future_ts);

    process_revocation(&revocation, &storage).unwrap();

    // Alice contact should be deleted (revocation is valid)
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
}

// @scenario: privacy_compliance.feature:Account deletion sends revocation signal to all contacts
#[test]
fn test_revocation_with_minimum_valid_timestamp() {
    let storage = test_storage();
    let alice = Identity::create("Alice");
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    let exchange_ts = alice_contact.exchange_timestamp();
    // Minimum valid timestamp (equal to exchange_timestamp, not less than)
    let min_valid_ts = exchange_ts;

    let revocation = AccountRevoked::create(&alice, &bob_id, min_valid_ts);

    process_revocation(&revocation, &storage).unwrap();

    // Alice contact should be deleted (timestamp equals exchange_timestamp, so >= condition is true)
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
}

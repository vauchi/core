// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the identity revocation protocol.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Identity deletion sends revocation signal to all contacts"
//!   - "Revocation signal is cryptographically authenticated"
//!   - "Spoofed revocation signal is rejected"
//!   - "Card update arriving after revocation is discarded"
//!   - "Replayed revocation for re-established contact is rejected"

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::network::message::IdentityRevoked;
use vauchi_core::network::revocation::{
    REVOCATION_BLOB_MAGIC, REVOCATION_DOMAIN_SEPARATOR, canonical_revocation_bytes,
    decode_revocation_blob, encode_revocation_blob, process_revocation,
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
        0,
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    // Pinned non-zero exchange timestamp — the stale-revocation test
    // depends on `exchange_timestamp > revocation_timestamp` for the
    // replay check, so 0 would defeat that comparison.
    Contact::from_exchange(pk, card, shared_key, 1_700_000_000)
}

// === Domain Separator ===

// @internal
#[test]
fn test_domain_separator_is_25_bytes() {
    assert_eq!(REVOCATION_DOMAIN_SEPARATOR.len(), 25);
    assert_eq!(REVOCATION_DOMAIN_SEPARATOR, b"vauchi-account-revoked-v1");
}

// === Canonical Signature ===

// @internal
#[test]
fn test_canonical_bytes_length() {
    let sender_id = [0x01u8; 32];
    let recipient_id = [0x02u8; 32];
    let timestamp: u64 = 1700000000;

    let bytes = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    assert_eq!(bytes.len(), 97); // 25 + 32 + 32 + 8
}

// @internal
#[test]
fn test_canonical_bytes_deterministic() {
    let sender_id = [0xAAu8; 32];
    let recipient_id = [0xBBu8; 32];
    let timestamp: u64 = 1700000000;

    let a = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    let b = canonical_revocation_bytes(&sender_id, &recipient_id, timestamp);
    assert_eq!(a, b);
}

// @internal
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

// @internal
#[test]
fn test_canonical_bytes_starts_with_domain_separator() {
    let bytes = canonical_revocation_bytes(&[0u8; 32], &[0u8; 32], 0);
    assert!(bytes.starts_with(REVOCATION_DOMAIN_SEPARATOR));
}

// === IdentityRevoked Message ===

// @scenario: privacy_compliance :: Revocation signal is cryptographically authenticated
// @internal
#[test]
fn test_identity_revoked_sign_and_verify() {
    let identity = Identity::create("Alice Test", 0);
    let recipient_pk = [0xBBu8; 32];
    let recipient_id = hex::encode(recipient_pk);
    let timestamp = 1700000000u64;

    let revoked = IdentityRevoked::create(&identity, &recipient_id, timestamp);

    assert!(revoked.verify(identity.signing_public_key()));
}

// @scenario: privacy_compliance :: Spoofed revocation signal is rejected
// @internal
#[test]
fn test_identity_revoked_rejects_tampered_timestamp() {
    let identity = Identity::create("Alice Test", 0);
    let recipient_pk = [0xBBu8; 32];
    let recipient_id = hex::encode(recipient_pk);

    let mut revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    revoked.timestamp = 1700000001;

    assert!(!revoked.verify(identity.signing_public_key()));
}

// @scenario: privacy_compliance :: Spoofed revocation signal is rejected
// @internal
#[test]
fn test_identity_revoked_rejects_wrong_key() {
    let identity = Identity::create("Alice", 0);
    let other_identity = Identity::create("Mallory", 0);
    let recipient_id = hex::encode([0xBBu8; 32]);

    let revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    assert!(!revoked.verify(other_identity.signing_public_key()));
}

// @internal
#[test]
fn test_identity_revoked_serialization_roundtrip() {
    let identity = Identity::create("Alice", 0);
    let recipient_id = hex::encode([0xCCu8; 32]);

    let revoked = IdentityRevoked::create(&identity, &recipient_id, 1700000000);

    let json = serde_json::to_string(&revoked).unwrap();
    let deserialized: IdentityRevoked = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.sender_id, revoked.sender_id);
    assert_eq!(deserialized.recipient_id, revoked.recipient_id);
    assert_eq!(deserialized.timestamp, revoked.timestamp);
    assert_eq!(deserialized.signature, revoked.signature);
}

// === Revocation Processing ===

// @scenario: privacy_compliance :: Identity deletion sends revocation signal to all contacts
// @internal
#[test]
fn test_process_revocation_deletes_contact_and_records_tombstone() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    // We are Bob. Store Alice as our contact.
    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Use a future timestamp (must be >= exchange_timestamp to not be stale)
    let future_ts = alice_contact.exchange_timestamp().unwrap() + 1;
    let revoked = IdentityRevoked::create(&identity, &bob_id, future_ts);

    let result = process_revocation(&revoked, &storage);
    result.expect("expected success");

    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());

    assert!(storage.is_sender_revoked(alice_contact.id()).unwrap());
}

// @scenario: privacy_compliance :: Spoofed revocation signal is rejected
// @internal
#[test]
fn test_process_revocation_rejects_invalid_signature() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let mallory = Identity::create("Mallory", 0);
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    let future_ts = alice_contact.exchange_timestamp().unwrap() + 1;
    let mut spoofed = IdentityRevoked::create(&mallory, &bob_id, future_ts);
    spoofed.sender_id = alice_contact.id().to_string().into();

    let result = process_revocation(&spoofed, &storage);
    result.expect("expected success"); // No error, just a no-op

    // Alice's contact should still exist (signature didn't verify)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

// @scenario: privacy_compliance :: Replayed revocation for re-established contact is rejected
// @internal
#[test]
fn test_process_revocation_stale_rejected() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Revocation with timestamp 0 (before any possible exchange)
    let stale_revoked = IdentityRevoked::create(&identity, &bob_id, 0);

    let result = process_revocation(&stale_revoked, &storage);
    result.expect("expected success");

    // Alice's contact should still exist (stale revocation ignored)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

// @internal
#[test]
fn test_process_revocation_unknown_sender_noop() {
    let storage = test_storage();
    let identity = Identity::create("Unknown", 0);
    let bob_id = hex::encode([0xBBu8; 32]);

    let revoked = IdentityRevoked::create(&identity, &bob_id, 1700000000);

    // No contact stored for sender — should be a no-op
    let result = process_revocation(&revoked, &storage);
    result.expect("expected success");
}

// @scenario: privacy_compliance :: Card update arriving after revocation is discarded
// @internal
#[test]
fn test_update_after_revocation_discarded_via_tombstone() {
    let storage = test_storage();

    storage
        .record_revoked_sender("alice_id", 1700000000)
        .unwrap();

    assert!(storage.is_sender_revoked("alice_id").unwrap());
}

// @scenario: privacy_compliance :: Identity deletion sends revocation signal to all contacts
// @internal
#[test]
fn test_revocation_only_deletes_matching_sender() {
    let storage = test_storage();
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);

    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    // Store Alice (with Alice's public key)
    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    // Bob revokes, using Alice's ID as recipient (Bob is the one sending revocation)
    let revocation = IdentityRevoked::create(&bob, &bob_id, 1700000000);

    process_revocation(&revocation, &storage).unwrap();

    // Alice should still be there (revocation was from unknown sender)
    storage
        .load_contact(alice_contact.id())
        .unwrap()
        .expect("expected Some");
}

// @scenario: security :: Tampered exchange data is rejected
// @internal
#[test]
fn test_revocation_with_future_timestamp() {
    let storage = test_storage();
    let alice = Identity::create("Alice", 0);
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    let exchange_ts = alice_contact.exchange_timestamp().unwrap();
    let future_ts = exchange_ts + 10000;

    let revocation = IdentityRevoked::create(&alice, &bob_id, future_ts);

    process_revocation(&revocation, &storage).unwrap();

    // Alice contact should be deleted (revocation is valid)
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
}

// @scenario: privacy_compliance :: Identity deletion sends revocation signal to all contacts
// @internal
#[test]
fn test_revocation_with_minimum_valid_timestamp() {
    let storage = test_storage();
    let alice = Identity::create("Alice", 0);
    let bob_pk = [0xBBu8; 32];
    let bob_id = hex::encode(bob_pk);

    let alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();

    let exchange_ts = alice_contact.exchange_timestamp().unwrap();
    // Minimum valid timestamp (equal to exchange_timestamp, not less than)
    let min_valid_ts = exchange_ts;

    let revocation = IdentityRevoked::create(&alice, &bob_id, min_valid_ts);

    process_revocation(&revocation, &storage).unwrap();

    // Alice contact should be deleted (timestamp equals exchange_timestamp, so >= condition is true)
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
}

// @scenario: privacy_compliance :: Identity deletion sends revocation signal to all contacts
/// Imported contacts have UUID IDs (not hex-encoded public keys).
/// `IdentityRevoked::create` must not panic for non-hex recipient IDs.
/// The resulting message is meaningless (imported contacts don't participate
/// in the relay protocol), but the deletion/shred flow must not crash.
// @internal
#[test]
fn test_identity_revoked_handles_uuid_recipient_id() {
    let identity = Identity::create("Alice", 0);
    let uuid_id = "550e8400-e29b-41d4-a716-446655440000";

    // Must not panic — imported contacts reach this code path via
    // execute_deletion() and ShredManager which iterate all contacts.
    let revoked = IdentityRevoked::create(&identity, uuid_id, 1700000000);

    // The message is created but verify() rejects it (non-hex recipient_id).
    assert!(
        !revoked.verify(identity.signing_public_key()),
        "revocation for non-hex recipient_id must not verify"
    );
}

// === Revocation wire format + receive routing (Finding 3) ===

// @scenario: privacy_compliance :: Revocation blob round-trips through encode/decode
// @internal
#[test]
fn test_revocation_blob_roundtrip() {
    let identity = Identity::create("Alice", 0);
    let bob_id = hex::encode([0xBBu8; 32]);
    let revoked = IdentityRevoked::create(&identity, &bob_id, 1234);

    let blob = encode_revocation_blob(&revoked);
    assert!(blob.starts_with(REVOCATION_BLOB_MAGIC));

    let decoded = decode_revocation_blob(&blob).expect("magic blob must decode");
    assert_eq!(decoded.sender_id, revoked.sender_id);
    assert_eq!(decoded.recipient_id, revoked.recipient_id);
    assert_eq!(decoded.timestamp, revoked.timestamp);
    assert_eq!(decoded.signature, revoked.signature);
}

// @scenario: privacy_compliance :: Non-revocation blobs are not misclassified
// @internal
#[test]
fn test_decode_rejects_non_revocation_blobs() {
    // An encrypted-update ciphertext (no magic) must decode to None so it is
    // never misrouted away from the update path.
    assert!(decode_revocation_blob(b"").is_none());
    assert!(decode_revocation_blob(b"VRV").is_none()); // shorter than the 4-byte magic
    assert!(decode_revocation_blob(&[0xABu8; 200]).is_none());

    // Correct magic but a too-short body fails to deserialize -> None (no panic).
    let mut bad = REVOCATION_BLOB_MAGIC.to_vec();
    bad.extend_from_slice(&[0xFFu8; 4]);
    assert!(decode_revocation_blob(&bad).is_none());
}

// @scenario: privacy_compliance :: process_revocation reports whether it acted
// @internal
#[test]
fn test_process_revocation_reports_outcome() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let bob_id = hex::encode([0xBBu8; 32]);

    // Unknown sender -> Ok(false) (no-op).
    let unknown = IdentityRevoked::create(&identity, &bob_id, 1);
    assert!(!process_revocation(&unknown, &storage).unwrap());

    // Known contact, valid signature -> Ok(true) and the contact is shredded.
    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();
    let future_ts = alice_contact.exchange_timestamp().unwrap() + 1;
    let valid = IdentityRevoked::create(&identity, &bob_id, future_ts);
    assert!(process_revocation(&valid, &storage).unwrap());
    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
}

// @scenario: privacy_compliance :: A received revocation blob shreds the contact
// @internal
#[test]
fn test_revocation_blob_decode_then_process_shreds_contact() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let bob_id = hex::encode([0xBBu8; 32]);
    let alice_contact = make_contact_with_pk(*identity.signing_public_key(), "Alice");
    storage.save_contact(&alice_contact).unwrap();
    let future_ts = alice_contact.exchange_timestamp().unwrap() + 1;

    // Simulate the receive path: encode -> blob -> decode -> process.
    let blob = encode_revocation_blob(&IdentityRevoked::create(&identity, &bob_id, future_ts));
    let decoded = decode_revocation_blob(&blob).expect("decode");
    assert!(process_revocation(&decoded, &storage).unwrap());

    assert!(storage.load_contact(alice_contact.id()).unwrap().is_none());
    assert!(storage.is_sender_revoked(alice_contact.id()).unwrap());
}

// End-to-end: Alice deletes her identity (GDPR), and Bob — her contact —
// receives the resulting delivery blob and crypto-shreds Alice. Proves the
// sender→blob→recipient→shred chain the broadcast wiring delivers.
// @scenario: privacy_compliance :: Identity deletion notifies contacts end-to-end
// @internal
#[test]
fn test_gdpr_deletion_revocation_shreds_at_recipient() {
    use base64::Engine;
    use vauchi_core::api::DeletionManager;

    let alice = Identity::create("Alice", 0);

    // Alice's side: Bob is her contact; schedule + execute deletion.
    let alice_storage = test_storage();
    let mut bob_card = ContactCard::new("Bob");
    bob_card
        .add_field(ContactField::new(
            FieldType::Email,
            "email",
            "bob@example.com",
            0,
        ))
        .unwrap();
    let alice_bob_contact = Contact::from_exchange(
        [0xBBu8; 32],
        bob_card,
        SymmetricKey::generate(),
        1_700_000_000,
    );
    alice_storage.save_contact(&alice_bob_contact).unwrap();

    let dm = DeletionManager::new(&alice_storage);
    dm.schedule_deletion_with_execute_at(0, 0).unwrap();
    let result = dm.execute_deletion(&alice).expect("execute deletion");
    assert_eq!(result.deliveries.len(), 1, "one delivery for Bob");
    let (_token, blob_b64) = &result.deliveries[0];

    // Bob's side: he has Alice as a contact, keyed by Alice's signing key.
    let bob_storage = test_storage();
    let bob_alice_contact = make_contact_with_pk(*alice.signing_public_key(), "Alice");
    bob_storage.save_contact(&bob_alice_contact).unwrap();
    assert!(
        bob_storage
            .load_contact(bob_alice_contact.id())
            .unwrap()
            .is_some()
    );

    // Bob receives the delivery blob → decodes → processes the revocation.
    let blob = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .unwrap();
    let rev = decode_revocation_blob(&blob).expect("delivery blob decodes");
    assert!(
        process_revocation(&rev, &bob_storage).expect("process"),
        "valid revocation must shred"
    );

    // Alice is crypto-shredded from Bob's contacts + tombstoned.
    assert!(
        bob_storage
            .load_contact(bob_alice_contact.id())
            .unwrap()
            .is_none(),
        "Alice must be shredded from Bob's contacts"
    );
    assert!(
        bob_storage
            .is_sender_revoked(bob_alice_contact.id())
            .unwrap(),
        "Alice must be tombstoned"
    );
}

// Spy test: broadcast_identity_revocations actually transmits the revocation
// blobs to the relay's /v2/send endpoint (the e2e shred test fed the blob
// straight to process_revocation, bypassing the real network hop).
// @scenario: privacy_compliance :: Revocation broadcast transmits to the relay
// @internal
#[test]
fn test_broadcast_identity_revocations_transmits_to_relay() {
    use crate::common::mock_relay::{CannedResponse, MockRelay};
    use base64::Engine;
    use vauchi_core::api::vauchi::VauchiBuilder;

    let mock = MockRelay::start();
    mock.queue(
        "send",
        CannedResponse::ok_json(br#"{"status":"ok","blob_id":"test-blob"}"#.to_vec()),
    );

    let dir = tempfile::tempdir().expect("temp dir");
    let mut wb = VauchiBuilder::new()
        .relay_url(mock.url())
        .storage_path(dir.path().join("vauchi.db").to_str().expect("utf-8 path"))
        .build()
        .expect("build vauchi");
    wb.create_identity("Alice").expect("create identity");

    // A pre-built delivery (token + base64 VRV1 blob), as DeletionResult yields.
    let revoker = Identity::create("Alice-revoked", 0);
    let recipient_id = hex::encode([0xBBu8; 32]);
    let rev = IdentityRevoked::create(&revoker, &recipient_id, 1_700_000_000);
    let token = "ab".repeat(32);
    let blob_b64 = base64::engine::general_purpose::STANDARD.encode(encode_revocation_blob(&rev));

    let sent = wb.broadcast_identity_revocations(&[(token.clone(), blob_b64.clone())]);
    assert_eq!(sent, 1, "the single delivery must be sent");

    // The relay received a /v2/send addressed to the token, carrying the blob.
    let req = mock.last_received();
    assert_eq!(req.path, "/v2/send");
    let body: serde_json::Value = serde_json::from_slice(&req.body).expect("JSON body");
    assert_eq!(body["recipient_id"], token);
    assert_eq!(body["ciphertext"], blob_b64);

    // The transmitted blob round-trips back to the revocation.
    let raw = base64::engine::general_purpose::STANDARD
        .decode(body["ciphertext"].as_str().unwrap())
        .unwrap();
    let decoded = decode_revocation_blob(&raw).expect("relay-delivered blob decodes");
    assert_eq!(decoded.recipient_id.as_str(), recipient_id);
}

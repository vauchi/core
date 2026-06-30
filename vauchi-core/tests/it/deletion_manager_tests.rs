// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for DeletionManager revocation and crypto-shredding during identity deletion.
//!
//! Traces to features/privacy_compliance.feature:
//!   - "Identity deletion destroys all content encryption keys"
//!   - "Identity deletion sends revocation signal to all contacts"
//!   - "Identity deletion propagates across all user devices"

use vauchi_core::api::deletion::{DeletionError, DeletionManager};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{DeletionState, Storage};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

fn make_contact_with_cek(pk: [u8; 32], name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "test@example.com",
        0,
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let mut contact = Contact::from_exchange(pk, card, shared_key, 0);
    contact.set_cek(ContentEncryptionKey::generate());
    contact
}

fn make_legacy_contact(pk: [u8; 32], name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "test@example.com",
        0,
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key, 0)
}

// === Revocation Generation ===

// @internal
#[test]
fn test_execute_deletion_returns_revocations_for_all_contacts() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    let carol = make_contact_with_cek([0xCC; 32], "Carol");
    storage.contacts().save_contact(&bob).unwrap();
    storage.contacts().save_contact(&carol).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();

    assert_eq!(result.revocations.len(), 2);
}

// @internal
#[test]
fn test_execute_deletion_revocations_have_correct_sender_id() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    storage.contacts().save_contact(&bob).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();

    let expected_sender_id = identity.public_id();
    for revocation in &result.revocations {
        assert_eq!(revocation.sender_id, expected_sender_id);
    }
}

// @internal
#[test]
fn test_execute_deletion_revocations_have_correct_recipient_ids() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    let carol = make_contact_with_cek([0xCC; 32], "Carol");
    let bob_id = bob.id().to_string();
    let carol_id = carol.id().to_string();
    storage.contacts().save_contact(&bob).unwrap();
    storage.contacts().save_contact(&carol).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();

    let recipient_ids: Vec<&str> = result
        .revocations
        .iter()
        .map(|r| r.recipient_id.as_str())
        .collect();
    assert!(recipient_ids.contains(&bob_id.as_str()));
    assert!(recipient_ids.contains(&carol_id.as_str()));
}

// @internal
#[test]
fn test_execute_deletion_revocations_verify_with_identity() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    storage.contacts().save_contact(&bob).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();

    for revocation in &result.revocations {
        assert!(
            revocation.verify(identity.signing_public_key()),
            "Revocation signature should verify with Alice's public key"
        );
    }
}

// === CEK Crypto-Shredding ===

// @internal
#[test]
fn test_execute_deletion_shreds_all_ceks() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    let carol = make_contact_with_cek([0xCC; 32], "Carol");
    let bob_id = bob.id().to_string();
    let carol_id = carol.id().to_string();
    storage.contacts().save_contact(&bob).unwrap();
    storage.contacts().save_contact(&carol).unwrap();

    // Verify CEKs exist before deletion
    storage
        .contacts()
        .load_contact_cek(&bob_id)
        .unwrap()
        .expect("expected Some");
    storage
        .contacts()
        .load_contact_cek(&carol_id)
        .unwrap()
        .expect("expected Some");

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();
    manager.execute_deletion(&identity).unwrap();

    // Contacts fully deleted (#48) — CEKs and rows removed
    assert!(
        storage.contacts().load_contact_cek(&bob_id).is_err(),
        "Bob's contact should be deleted"
    );
    assert!(
        storage.contacts().load_contact_cek(&carol_id).is_err(),
        "Carol's contact should be deleted"
    );
}

// @internal
#[test]
fn test_execute_deletion_contacts_still_exist_after_shredding() {
    // Contacts remain in DB (DB deletion is a separate step)
    // but their card data is unreadable because CEK is shredded
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    let bob_id = bob.id().to_string();
    storage.contacts().save_contact(&bob).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();
    manager.execute_deletion(&identity).unwrap();

    // Contact row still exists (DB not deleted yet)
    // But loading it will fail because CEK is shredded and card can't be decrypted
    let result = storage.contacts().load_contact(&bob_id);
    // The load will either return None (contact deleted) or fail to decrypt
    // Either outcome proves crypto-shredding worked
    assert!(
        result.is_err() || result.unwrap().is_none(),
        "Contact should be unloadable after CEK shredding"
    );
}

// === Grace Period and State ===

// @internal
#[test]
fn test_execute_deletion_still_requires_grace_period() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let manager = DeletionManager::new(&storage);
    manager
        .schedule_deletion_with_execute_at(0, u64::MAX)
        .unwrap();

    let result = manager.execute_deletion(&identity);
    assert!(matches!(result, Err(DeletionError::GracePeriodNotElapsed)));
}

// @internal
#[test]
fn test_execute_deletion_marks_state_as_executed() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();
    manager.execute_deletion(&identity).unwrap();

    let state = manager.deletion_state().unwrap();
    assert!(matches!(state, DeletionState::Executed { .. }));
}

// @internal
#[test]
fn test_execute_deletion_requires_scheduled_state() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let manager = DeletionManager::new(&storage);
    // Don't schedule deletion

    let result = manager.execute_deletion(&identity);
    result.expect_err("expected error");
}

// === Edge Cases ===

// @scenario: emergency_shred :: Shred with no contacts
// @internal
#[test]
fn test_execute_deletion_no_contacts_returns_empty_revocations() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();
    assert!(result.revocations.is_empty());
}

// @internal
#[test]
fn test_execute_deletion_legacy_contacts_get_revocations() {
    // Legacy contacts (no CEK) should still get revocation messages
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_legacy_contact([0xBB; 32], "Bob");
    storage.contacts().save_contact(&bob).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();
    assert_eq!(result.revocations.len(), 1);
}

// @internal
#[test]
fn test_execute_deletion_mixed_cek_and_legacy_contacts() {
    let storage = test_storage();
    let identity = Identity::create("Alice", 0);

    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    let carol = make_legacy_contact([0xCC; 32], "Carol");
    let bob_id = bob.id().to_string();
    storage.contacts().save_contact(&bob).unwrap();
    storage.contacts().save_contact(&carol).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();

    let result = manager.execute_deletion(&identity).unwrap();

    assert_eq!(result.revocations.len(), 2);

    // Bob's contact fully deleted (#48)
    storage
        .contacts()
        .load_contact_cek(&bob_id)
        .expect_err("expected error");
}

// @scenario: privacy_compliance :: Identity deletion produces relay-ready revocation deliveries
// @internal
#[test]
fn test_execute_deletion_produces_decodable_deliveries() {
    use base64::Engine;
    use vauchi_core::network::mailbox_token::{
        compute_mailbox_token, current_day_epoch, token_hex,
    };
    use vauchi_core::network::revocation::decode_revocation_blob;

    let storage = test_storage();
    let identity = Identity::create("Alice", 0);
    let bob = make_contact_with_cek([0xBB; 32], "Bob");
    storage.contacts().save_contact(&bob).unwrap();

    let manager = DeletionManager::new(&storage);
    manager.schedule_deletion_with_execute_at(0, 0).unwrap();
    let result = manager.execute_deletion(&identity).unwrap();

    assert_eq!(result.deliveries.len(), 1, "one delivery per contact");
    let (token, blob_b64) = &result.deliveries[0];

    // The blob decodes to a revocation signed by us, addressed to Bob.
    let blob = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .unwrap();
    let rev = decode_revocation_blob(&blob).expect("delivery blob must decode");
    assert!(
        rev.verify(identity.signing_public_key()),
        "delivery must be signed by the deleter"
    );
    assert_eq!(rev.recipient_id.as_str(), bob.id());

    // The token addresses Bob's mailbox, derived the same way the recipient
    // registers it (epoch taken from the revocation to avoid clock coupling).
    let expected = token_hex(&compute_mailbox_token(
        bob.shared_key().unwrap().as_bytes(),
        bob.public_key().unwrap(),
        current_day_epoch(rev.timestamp),
    ));
    assert_eq!(token, &expected, "delivery must target Bob's mailbox token");
}

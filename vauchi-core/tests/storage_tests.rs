// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for storage
//! Extracted from mod.rs

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::*;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_contact(name: &str) -> Contact {
    let public_key = [0u8; 32];
    let mut card = ContactCard::new(name);
    let _ = card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        &format!("{}@example.com", name.to_lowercase()),
    ));
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

#[test]
fn test_storage_save_load_contact() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();

    // Save
    storage.save_contact(&contact).unwrap();

    // Load
    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();

    assert_eq!(loaded.id(), contact.id());
    assert_eq!(loaded.display_name(), "Alice");
    assert_eq!(loaded.card().fields().len(), 1);
}

#[test]
fn test_storage_list_contacts() {
    let storage = create_test_storage();

    // Create contacts with different public keys
    let mut contact1 = create_test_contact("Alice");
    let mut contact2 = create_test_contact("Bob");

    // Give them different IDs by using different public keys
    let pk1 = [1u8; 32];
    let pk2 = [2u8; 32];
    contact1 = Contact::from_exchange(pk1, contact1.card().clone(), SymmetricKey::generate());
    contact2 = Contact::from_exchange(pk2, contact2.card().clone(), SymmetricKey::generate());

    storage.save_contact(&contact1).unwrap();
    storage.save_contact(&contact2).unwrap();

    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 2);
}

#[test]
fn test_storage_delete_contact() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();
    assert!(
        storage.load_contact(&contact_id).unwrap().is_some(),
        "expected Some value"
    );

    let deleted = storage.delete_contact(&contact_id).unwrap();
    assert!(deleted);

    assert!(storage.load_contact(&contact_id).unwrap().is_none());
}

#[test]
fn test_storage_contact_not_found() {
    let storage = create_test_storage();
    let result = storage.load_contact("nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_storage_save_load_own_card() {
    let storage = create_test_storage();

    let mut card = ContactCard::new("My Card");
    let _ = card.add_field(ContactField::new(FieldType::Phone, "mobile", "+1234567890"));

    storage.save_own_card(&card).unwrap();

    let loaded = storage.load_own_card().unwrap().unwrap();
    assert_eq!(loaded.display_name(), "My Card");
    assert_eq!(loaded.fields().len(), 1);
}

#[test]
fn test_storage_own_card_not_found() {
    let storage = create_test_storage();
    let result = storage.load_own_card().unwrap();
    assert!(result.is_none());
}

#[test]
fn test_storage_pending_updates() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.save_contact(&contact).unwrap();

    let update = PendingUpdate {
        id: "update-1".to_string(),
        contact_id: contact.id().to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3, 4],
        created_at: 12345,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    };

    storage.queue_update(&update).unwrap();

    let pending = storage.get_pending_updates(contact.id()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "update-1");
    assert_eq!(pending[0].payload, vec![1, 2, 3, 4]);
}

#[test]
fn test_storage_mark_update_sent() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.save_contact(&contact).unwrap();

    let update = PendingUpdate {
        id: "update-1".to_string(),
        contact_id: contact.id().to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: 12345,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    };

    storage.queue_update(&update).unwrap();
    assert_eq!(storage.count_pending_updates(contact.id()).unwrap(), 1);

    storage.mark_update_sent("update-1").unwrap();
    assert_eq!(storage.count_pending_updates(contact.id()).unwrap(), 0);
}

#[test]
fn test_storage_update_status() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.save_contact(&contact).unwrap();

    let update = PendingUpdate {
        id: "update-1".to_string(),
        contact_id: contact.id().to_string(),
        update_type: "card_update".to_string(),
        payload: vec![1, 2, 3],
        created_at: 12345,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
    };

    storage.queue_update(&update).unwrap();

    // Update to failed status
    storage
        .update_pending_status(
            "update-1",
            UpdateStatus::Failed {
                error: "Connection failed".to_string(),
                retry_at: 99999,
            },
            1,
        )
        .unwrap();

    let pending = storage.get_pending_updates(contact.id()).unwrap();
    assert_eq!(pending[0].retry_count, 1);
    assert!(matches!(pending[0].status, UpdateStatus::Failed { .. }));
}

#[test]
fn test_storage_save_load_ratchet_state() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.save_contact(&contact).unwrap();

    // Create ratchet state (as initiator)
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    // Save ratchet state
    storage
        .save_ratchet_state(contact.id(), &ratchet, true)
        .unwrap();

    // Load ratchet state
    let (loaded, is_initiator) = storage.load_ratchet_state(contact.id()).unwrap().unwrap();

    assert!(is_initiator);
    assert_eq!(loaded.dh_generation(), ratchet.dh_generation());
    assert_eq!(loaded.our_public_key(), ratchet.our_public_key());
}

#[test]
fn test_storage_ratchet_state_encryption() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.save_contact(&contact).unwrap();

    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    // Encrypt a message to advance the ratchet
    let _msg = ratchet.encrypt(b"test message").unwrap();

    // Save and load
    storage
        .save_ratchet_state(contact.id(), &ratchet, true)
        .unwrap();
    let (mut loaded, _) = storage.load_ratchet_state(contact.id()).unwrap().unwrap();

    // The loaded ratchet should be able to continue encrypting
    let msg2 = loaded.encrypt(b"another message").unwrap();
    assert!(!msg2.ciphertext.is_empty());
}

#[test]
fn test_storage_ratchet_deleted_with_contact() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    storage
        .save_ratchet_state(&contact_id, &ratchet, true)
        .unwrap();

    // Verify ratchet exists
    assert!(
        storage.load_ratchet_state(&contact_id).unwrap().is_some(),
        "expected Some value"
    );

    // Delete contact
    storage.delete_contact(&contact_id).unwrap();

    // Ratchet should also be deleted
    assert!(storage.load_ratchet_state(&contact_id).unwrap().is_none());
}

#[test]
fn test_storage_ratchet_not_found() {
    let storage = create_test_storage();
    let result = storage.load_ratchet_state("nonexistent").unwrap();
    assert!(result.is_none());
}

/// SP-9 #126: Ratchet states use per-contact derived encryption keys.
/// Verifies that two contacts' ratchet states are independently encrypted
/// and can be loaded back correctly.
#[test]
fn test_storage_ratchet_per_contact_key_isolation() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();

    // Use distinct public keys to create separate contacts
    let alice = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let bob = Contact::from_exchange([2u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    storage.save_contact(&alice).unwrap();
    storage.save_contact(&bob).unwrap();

    // Create and save ratchet state for Alice
    let secret_a = SymmetricKey::generate();
    let dh_a = X3DHKeyPair::generate();
    let ratchet_a =
        DoubleRatchetState::initialize_initiator(&secret_a, *dh_a.public_key()).unwrap();
    storage
        .save_ratchet_state(alice.id(), &ratchet_a, true)
        .unwrap();

    // Create and save ratchet state for Bob
    let secret_b = SymmetricKey::generate();
    let dh_b = X3DHKeyPair::generate();
    let ratchet_b =
        DoubleRatchetState::initialize_initiator(&secret_b, *dh_b.public_key()).unwrap();
    storage
        .save_ratchet_state(bob.id(), &ratchet_b, false)
        .unwrap();

    // Load Alice's ratchet — must succeed and match
    let (loaded_a, is_init_a) = storage.load_ratchet_state(alice.id()).unwrap().unwrap();
    assert!(is_init_a);
    assert_eq!(loaded_a.dh_generation(), ratchet_a.dh_generation());
    assert_eq!(loaded_a.our_public_key(), ratchet_a.our_public_key());

    // Load Bob's ratchet — must succeed and match
    let (loaded_b, is_init_b) = storage.load_ratchet_state(bob.id()).unwrap().unwrap();
    assert!(!is_init_b);
    assert_eq!(loaded_b.dh_generation(), ratchet_b.dh_generation());
    assert_eq!(loaded_b.our_public_key(), ratchet_b.our_public_key());
}

#[test]
fn test_storage_save_load_device_info() {
    let storage = create_test_storage();

    let device_id = [0x42u8; 32];
    let device_index = 0u32;
    let device_name = "My Phone";
    let created_at = 1234567890u64;

    // Initially no device info
    assert!(!storage.has_device_info().unwrap());

    // Save device info
    storage
        .save_device_info(&device_id, device_index, device_name, created_at)
        .unwrap();

    // Now has device info
    assert!(storage.has_device_info().unwrap());

    // Load and verify
    let (loaded_id, loaded_index, loaded_name, loaded_created) =
        storage.load_device_info().unwrap().unwrap();

    assert_eq!(loaded_id, device_id);
    assert_eq!(loaded_index, device_index);
    assert_eq!(loaded_name, device_name);
    assert_eq!(loaded_created, created_at);
}

#[test]
fn test_storage_device_info_update() {
    let storage = create_test_storage();

    storage
        .save_device_info(&[1u8; 32], 0, "Old Name", 100)
        .unwrap();
    storage
        .save_device_info(&[2u8; 32], 1, "New Name", 200)
        .unwrap();

    let (id, index, name, _) = storage.load_device_info().unwrap().unwrap();
    assert_eq!(id, [2u8; 32]);
    assert_eq!(index, 1);
    assert_eq!(name, "New Name");
}

#[test]
fn test_storage_save_load_device_registry() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device = DeviceInfo::derive(&master_seed, 0, "Primary".to_string());
    let registry = DeviceRegistry::new(device.to_registered(&master_seed), &signing_key);

    // Initially no registry
    assert!(!storage.has_device_registry().unwrap());

    // Save registry
    storage.save_device_registry(&registry).unwrap();

    // Now has registry
    assert!(storage.has_device_registry().unwrap());

    // Load and verify
    let loaded = storage.load_device_registry().unwrap().unwrap();
    assert_eq!(loaded.version(), registry.version());
    assert_eq!(loaded.active_count(), 1);
    assert!(loaded.verify(&signing_key.public_key()));
}

#[test]
fn test_storage_device_registry_roundtrip() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Primary".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Secondary".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    storage.save_device_registry(&registry).unwrap();
    let loaded = storage.load_device_registry().unwrap().unwrap();

    assert_eq!(loaded.version(), 2);
    assert_eq!(loaded.active_count(), 2);
}

// ============================================================
// Phase 1: Device Sync State Storage Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Offline changes sync when reconnected
/// Need to persist pending sync items between app restarts
#[test]
fn test_storage_save_load_device_sync_state() {
    use vauchi_core::sync::device_sync::{InterDeviceSyncState, SyncItem};

    let storage = create_test_storage();
    let device_id = [0x42u8; 32];

    // Create sync state with pending items
    let mut state = InterDeviceSyncState::new(device_id);
    state.queue_item(SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "test@example.com".to_string(),
        timestamp: 1000,
    });
    state.queue_item(SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 2000,
    });

    // Save
    storage.save_device_sync_state(&state).unwrap();

    // Load
    let loaded = storage.load_device_sync_state(&device_id).unwrap().unwrap();

    assert_eq!(loaded.device_id(), &device_id);
    assert_eq!(loaded.pending_items().len(), 2);
    assert_eq!(loaded.pending_items()[0].timestamp(), 1000);
    assert_eq!(loaded.pending_items()[1].timestamp(), 2000);
}

/// Test that we can list all device sync states
#[test]
fn test_storage_list_device_sync_states() {
    use vauchi_core::sync::device_sync::{InterDeviceSyncState, SyncItem};

    let storage = create_test_storage();

    let device_a = [0x41u8; 32];
    let device_b = [0x42u8; 32];

    let mut state_a = InterDeviceSyncState::new(device_a);
    state_a.queue_item(SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "a@test.com".to_string(),
        timestamp: 1000,
    });

    let mut state_b = InterDeviceSyncState::new(device_b);
    state_b.queue_item(SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+999".to_string(),
        timestamp: 2000,
    });

    storage.save_device_sync_state(&state_a).unwrap();
    storage.save_device_sync_state(&state_b).unwrap();

    let states = storage.list_device_sync_states().unwrap();
    assert_eq!(states.len(), 2);
}

/// Test version vector persistence for conflict detection
#[test]
fn test_storage_save_load_version_vector() {
    use vauchi_core::sync::device_sync::VersionVector;

    let storage = create_test_storage();

    let device_a = [0x41u8; 32];
    let device_b = [0x42u8; 32];

    let mut vector = VersionVector::new();
    vector.increment(&device_a);
    vector.increment(&device_a);
    vector.increment(&device_b);

    // Save
    storage.save_version_vector(&vector).unwrap();

    // Load
    let loaded = storage.load_version_vector().unwrap().unwrap();

    assert_eq!(loaded.get(&device_a), 2);
    assert_eq!(loaded.get(&device_b), 1);
}

/// Test that version vector updates correctly
#[test]
fn test_storage_version_vector_update() {
    use vauchi_core::sync::device_sync::VersionVector;

    let storage = create_test_storage();

    let device_a = [0x41u8; 32];

    let mut vector1 = VersionVector::new();
    vector1.increment(&device_a);
    storage.save_version_vector(&vector1).unwrap();

    // Update with new version
    let mut vector2 = VersionVector::new();
    vector2.increment(&device_a);
    vector2.increment(&device_a);
    vector2.increment(&device_a);
    storage.save_version_vector(&vector2).unwrap();

    let loaded = storage.load_version_vector().unwrap().unwrap();
    assert_eq!(loaded.get(&device_a), 3);
}

/// Test that recovery_trusted flag persists through save/load
#[test]
fn test_storage_recovery_trusted_persistence() {
    let storage = create_test_storage();

    let pk = [0xAAu8; 32];
    let card = ContactCard::new("Trusted Friend");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = vauchi_core::contact::VisibilityRules::new();

    let mut contact = Contact::from_sync_data_full(
        pk,
        card,
        shared_key,
        1234567890,
        false,
        visibility_rules,
        false, // hidden
        false, // blocked
        true,  // recovery_trusted
    );

    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(loaded.is_recovery_trusted());

    // Now untrust and save again
    contact.untrust_for_recovery().unwrap();
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(!loaded.is_recovery_trusted());
}

// ============================================================
// Coverage gap tests — contact_limit, delta_version
// ============================================================

/// Test get_contact_limit returns default 10_000
// @scenario: contacts_management :: Contact limits
#[test]
fn test_get_contact_limit_default() {
    let storage = create_test_storage();
    let limit = storage.get_contact_limit().unwrap();
    assert_eq!(limit, 10_000);
}

/// Test last_delta_version defaults to 0 for new contact
// @scenario: sync_updates :: Delta sync versioning
#[test]
fn test_last_delta_version_default() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    let version = storage.last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 0);
}

/// Test record_delta_version and last_delta_version roundtrip
// @scenario: sync_updates :: Delta sync versioning
#[test]
fn test_record_and_load_delta_version() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    storage.record_delta_version(&contact_id, 42).unwrap();
    let version = storage.last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 42);

    // Update to higher version
    storage.record_delta_version(&contact_id, 100).unwrap();
    let version = storage.last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 100);
}

/// Test last_delta_version fails for nonexistent contact
// @scenario: sync_updates :: Delta sync versioning
#[test]
fn test_last_delta_version_nonexistent_contact() {
    let storage = create_test_storage();
    let result = storage.last_delta_version("nonexistent");
    assert!(result.is_err(), "expected error");
}

/// Test wipe_device_data clears device_info and sync state
// @scenario: identity_management :: Account deletion
#[test]
fn test_wipe_device_data() {
    let storage = create_test_storage();

    // Save device info
    let device_id = [0x42u8; 32];
    storage
        .save_device_info(&device_id, 0, "Test Device", 1000)
        .unwrap();
    assert!(storage.has_device_info().unwrap());

    // Wipe
    storage.wipe_device_data().unwrap();

    // Device info should be gone
    assert!(!storage.has_device_info().unwrap());
}

/// Test is_replay_nonce detects duplicates
// @scenario: security :: Replay attack prevention
#[test]
fn test_is_replay_nonce() {
    let storage = create_test_storage();

    let nonce = [0xAAu8; 32];

    // Fresh nonce is not a replay
    assert!(!storage.is_replay_nonce("contact-1", &nonce).unwrap());

    // Record the nonce
    storage
        .save_replay_nonce("contact-1", &nonce, 1000)
        .unwrap();

    // Now it's a replay
    assert!(storage.is_replay_nonce("contact-1", &nonce).unwrap());

    // Same nonce for different contact is NOT a replay
    assert!(!storage.is_replay_nonce("contact-2", &nonce).unwrap());
}

/// Test cleanup_replay_nonces removes old entries
// @scenario: security :: Replay attack prevention
#[test]
fn test_cleanup_replay_nonces() {
    let storage = create_test_storage();

    let old_nonce = [0x11u8; 32];
    let new_nonce = [0x22u8; 32];

    storage.save_replay_nonce("c1", &old_nonce, 1000).unwrap();
    storage.save_replay_nonce("c1", &new_nonce, 5000).unwrap();

    // Cleanup nonces older than 3000
    let removed = storage.cleanup_replay_nonces(3000).unwrap();
    assert_eq!(removed, 1);

    // Old nonce should be gone
    assert!(!storage.is_replay_nonce("c1", &old_nonce).unwrap());
    // New nonce should remain
    assert!(storage.is_replay_nonce("c1", &new_nonce).unwrap());
}

/// Test load_device_registry_json returns structured JSON
// @scenario: identity_management :: GDPR data export
#[test]
fn test_load_device_registry_json() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();

    // No registry initially
    let json = storage.load_device_registry_json().unwrap();
    assert!(json.is_none());

    // Create and save a registry
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let device = DeviceInfo::derive(&master_seed, 0, "Test".to_string());
    let registry = DeviceRegistry::new(device.to_registered(&master_seed), &signing_key);
    storage.save_device_registry(&registry).unwrap();

    // Load as JSON
    let json = storage.load_device_registry_json().unwrap();
    assert!(json.is_some(), "expected Some value");
    let json_str = json.unwrap();
    assert!(json_str.contains("device_id"));
}

// ============================================================
// proposal_trusted storage persistence (Task 3)
// ============================================================

/// Test that proposal_trusted defaults to false on a new contact.
// @scenario: contacts_management :: Contact trust management
#[test]
fn test_proposal_trusted_defaults_false() {
    let contact = create_test_contact("Default User");
    assert!(
        !contact.is_proposal_trusted(),
        "proposal_trusted must default to false"
    );
}

/// Test that proposal_trusted = true survives a save/load roundtrip.
// @scenario: contacts_management :: Contact trust management
#[test]
fn test_proposal_trusted_storage_roundtrip() {
    let storage = create_test_storage();

    let pk = [0xBBu8; 32];
    let card = ContactCard::new("Proposal Friend");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = vauchi_core::contact::VisibilityRules::new();

    let mut contact = Contact::from_sync_data_full(
        pk,
        card,
        shared_key,
        1234567890,
        false,
        visibility_rules,
        false, // hidden
        false, // blocked
        false, // recovery_trusted
    );
    contact.set_proposal_trusted(true).unwrap();

    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(
        loaded.is_proposal_trusted(),
        "proposal_trusted must survive save/load roundtrip"
    );
}

/// Test that proposal_trusted = false is correctly persisted and reloaded.
// @scenario: contacts_management :: Contact trust management
#[test]
fn test_proposal_trusted_false_persists() {
    let storage = create_test_storage();

    let pk = [0xCCu8; 32];
    let card = ContactCard::new("Untrusted Contact");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = vauchi_core::contact::VisibilityRules::new();

    let mut contact = Contact::from_sync_data_full(
        pk,
        card,
        shared_key,
        1234567890,
        false,
        visibility_rules,
        false, // hidden
        false, // blocked
        false, // recovery_trusted
    );
    // Explicitly leave proposal_trusted as false (the default)

    let contact_id = contact.id().to_string();
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(
        !loaded.is_proposal_trusted(),
        "proposal_trusted = false must persist correctly"
    );

    // Toggle true → save → load → false
    contact.set_proposal_trusted(true).unwrap();
    storage.save_contact(&contact).unwrap();
    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(loaded.is_proposal_trusted());

    contact.set_proposal_trusted(false).unwrap();
    storage.save_contact(&contact).unwrap();
    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(
        !loaded.is_proposal_trusted(),
        "proposal_trusted must be updatable back to false"
    );
}

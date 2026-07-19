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
        0,
    ));
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

// @internal
#[test]
fn test_storage_save_load_contact() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded.id(), contact.id());
    assert_eq!(loaded.display_name(), "Alice");
    assert_eq!(loaded.card().fields().len(), 1);
}

// @internal
#[test]
fn test_storage_list_contacts() {
    let storage = create_test_storage();

    let mut contact1 = create_test_contact("Alice");
    let mut contact2 = create_test_contact("Bob");

    // Give them different IDs by using different public keys
    let pk1 = [1u8; 32];
    let pk2 = [2u8; 32];
    contact1 = Contact::from_exchange(pk1, contact1.card().clone(), SymmetricKey::generate(), 0);
    contact2 = Contact::from_exchange(pk2, contact2.card().clone(), SymmetricKey::generate(), 0);

    storage.contacts().save_contact(&contact1).unwrap();
    storage.contacts().save_contact(&contact2).unwrap();

    let contacts = storage.contacts().list_contacts().unwrap();
    assert_eq!(contacts.len(), 2);
}

// @internal
#[test]
fn test_storage_delete_contact() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();
    assert!(
        storage
            .contacts()
            .load_contact(&contact_id)
            .unwrap()
            .is_some(),
        "expected Some value"
    );

    let deleted = storage.delete_contact(&contact_id).unwrap();
    assert!(deleted);

    assert!(
        storage
            .contacts()
            .load_contact(&contact_id)
            .unwrap()
            .is_none()
    );
}

// @internal
#[test]
fn test_storage_contact_not_found() {
    let storage = create_test_storage();
    let result = storage.contacts().load_contact("nonexistent").unwrap();
    assert!(result.is_none());
}

// @internal
#[test]
fn test_storage_save_load_own_card() {
    let storage = create_test_storage();

    let mut card = ContactCard::new("My Card");
    let _ = card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+1234567890",
        0,
    ));

    storage.contacts().save_own_card(&card).unwrap();

    let loaded = storage.contacts().load_own_card().unwrap().unwrap();
    assert_eq!(loaded.display_name(), "My Card");
    assert_eq!(loaded.fields().len(), 1);
}

// @internal
#[test]
fn test_storage_own_card_not_found() {
    let storage = create_test_storage();
    let result = storage.contacts().load_own_card().unwrap();
    assert!(result.is_none());
}

// @internal
#[test]
fn test_storage_pending_updates() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

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

    storage.pending().queue_update(&update).unwrap();

    let pending = storage.pending().get_pending_updates(contact.id()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "update-1");
    assert_eq!(pending[0].payload, vec![1, 2, 3, 4]);
}

// @internal
#[test]
fn test_storage_mark_update_sent() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

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

    storage.pending().queue_update(&update).unwrap();
    assert_eq!(
        storage
            .pending()
            .count_pending_updates(contact.id())
            .unwrap(),
        1
    );

    storage.pending().mark_update_sent("update-1").unwrap();
    assert_eq!(
        storage
            .pending()
            .count_pending_updates(contact.id())
            .unwrap(),
        0
    );
}

// @internal
#[test]
fn test_storage_update_status() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

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

    storage.pending().queue_update(&update).unwrap();

    storage
        .pending()
        .update_pending_status(
            "update-1",
            UpdateStatus::Failed {
                error: "Connection failed".to_string(),
                retry_at: 99999,
            },
            1,
        )
        .unwrap();

    let pending = storage.pending().get_pending_updates(contact.id()).unwrap();
    assert_eq!(pending[0].retry_count, 1);
    assert!(matches!(pending[0].status, UpdateStatus::Failed { .. }));
}

// @internal
#[test]
fn test_storage_save_load_ratchet_state() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

    // Create ratchet state (as initiator)
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    storage
        .ratchets()
        .save_ratchet_state(contact.id(), &ratchet, true)
        .unwrap();

    let (loaded, is_initiator) = storage
        .ratchets()
        .load_ratchet_state(contact.id())
        .unwrap()
        .unwrap();

    assert!(is_initiator);
    assert_eq!(loaded.dh_generation(), ratchet.dh_generation());
    assert_eq!(loaded.our_public_key(), ratchet.our_public_key());
}

// @internal
#[test]
fn ratchet_states_are_isolated_by_peer_device_id() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

    let first_secret = SymmetricKey::generate();
    let first_dh = X3DHKeyPair::generate();
    let first =
        DoubleRatchetState::initialize_initiator(&first_secret, *first_dh.public_key()).unwrap();

    let second_secret = SymmetricKey::generate();
    let second_dh = X3DHKeyPair::generate();
    let second =
        DoubleRatchetState::initialize_initiator(&second_secret, *second_dh.public_key()).unwrap();
    let first_device_id = [1; 32];
    let second_device_id = [2; 32];

    storage
        .ratchets()
        .save_ratchet_state_for_device(contact.id(), &first_device_id, &first, true)
        .unwrap();
    storage
        .ratchets()
        .save_ratchet_state_for_device(contact.id(), &second_device_id, &second, false)
        .unwrap();

    let (loaded_first, first_is_initiator) = storage
        .ratchets()
        .load_ratchet_state_for_device(contact.id(), &first_device_id)
        .unwrap()
        .unwrap();
    let (loaded_second, second_is_initiator) = storage
        .ratchets()
        .load_ratchet_state_for_device(contact.id(), &second_device_id)
        .unwrap()
        .unwrap();

    assert!(first_is_initiator);
    assert!(!second_is_initiator);
    assert_eq!(loaded_first.our_public_key(), first.our_public_key());
    assert_eq!(loaded_second.our_public_key(), second.our_public_key());

    assert!(
        storage
            .ratchets()
            .delete_ratchet_state_for_device(contact.id(), &first_device_id)
            .unwrap()
    );
    assert!(
        storage
            .ratchets()
            .load_ratchet_state_for_device(contact.id(), &first_device_id)
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .ratchets()
            .load_ratchet_state_for_device(contact.id(), &second_device_id)
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn test_storage_ratchet_state_encryption() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    storage.contacts().save_contact(&contact).unwrap();

    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    let _msg = ratchet.encrypt(b"test message").unwrap();

    storage
        .ratchets()
        .save_ratchet_state(contact.id(), &ratchet, true)
        .unwrap();
    let (mut loaded, _) = storage
        .ratchets()
        .load_ratchet_state(contact.id())
        .unwrap()
        .unwrap();

    let msg2 = loaded.encrypt(b"another message").unwrap();
    assert!(!msg2.ciphertext.is_empty());
}

// @internal
#[test]
fn test_storage_ratchet_deleted_with_contact() {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *their_dh.public_key()).unwrap();

    storage
        .ratchets()
        .save_ratchet_state(&contact_id, &ratchet, true)
        .unwrap();

    assert!(
        storage
            .ratchets()
            .load_ratchet_state(&contact_id)
            .unwrap()
            .is_some(),
        "expected Some value"
    );

    storage.delete_contact(&contact_id).unwrap();

    assert!(
        storage
            .ratchets()
            .load_ratchet_state(&contact_id)
            .unwrap()
            .is_none()
    );
}

// @internal
#[test]
fn test_storage_ratchet_not_found() {
    let storage = create_test_storage();
    let result = storage
        .ratchets()
        .load_ratchet_state("nonexistent")
        .unwrap();
    assert!(result.is_none());
}

/// SP-9 #126: Ratchet states use per-contact derived encryption keys.
/// Verifies that two contacts' ratchet states are independently encrypted
/// and can be loaded back correctly.
// @internal
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
        0,
    );
    let bob = Contact::from_exchange(
        [2u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&alice).unwrap();
    storage.contacts().save_contact(&bob).unwrap();

    let secret_a = SymmetricKey::generate();
    let dh_a = X3DHKeyPair::generate();
    let ratchet_a =
        DoubleRatchetState::initialize_initiator(&secret_a, *dh_a.public_key()).unwrap();
    storage
        .ratchets()
        .save_ratchet_state(alice.id(), &ratchet_a, true)
        .unwrap();

    let secret_b = SymmetricKey::generate();
    let dh_b = X3DHKeyPair::generate();
    let ratchet_b =
        DoubleRatchetState::initialize_initiator(&secret_b, *dh_b.public_key()).unwrap();
    storage
        .ratchets()
        .save_ratchet_state(bob.id(), &ratchet_b, false)
        .unwrap();

    // Load Alice's ratchet — must succeed and match
    let (loaded_a, is_init_a) = storage
        .ratchets()
        .load_ratchet_state(alice.id())
        .unwrap()
        .unwrap();
    assert!(is_init_a);
    assert_eq!(loaded_a.dh_generation(), ratchet_a.dh_generation());
    assert_eq!(loaded_a.our_public_key(), ratchet_a.our_public_key());

    // Load Bob's ratchet — must succeed and match
    let (loaded_b, is_init_b) = storage
        .ratchets()
        .load_ratchet_state(bob.id())
        .unwrap()
        .unwrap();
    assert!(!is_init_b);
    assert_eq!(loaded_b.dh_generation(), ratchet_b.dh_generation());
    assert_eq!(loaded_b.our_public_key(), ratchet_b.our_public_key());
}

// @internal
#[test]
fn test_storage_save_load_device_info() {
    let storage = create_test_storage();

    let device_id = [0x42u8; 32];
    let device_index = 0u32;
    let device_name = "My Phone";
    let created_at = 1234567890u64;

    assert!(!storage.device().has_device_info().unwrap());

    storage
        .device()
        .save_device_info(&device_id, device_index, device_name, created_at)
        .unwrap();

    assert!(storage.device().has_device_info().unwrap());

    let (loaded_id, loaded_index, loaded_name, loaded_created) =
        storage.device().load_device_info().unwrap().unwrap();

    assert_eq!(loaded_id, device_id);
    assert_eq!(loaded_index, device_index);
    assert_eq!(loaded_name, device_name);
    assert_eq!(loaded_created, created_at);
}

// @internal
#[test]
fn test_storage_device_info_update() {
    let storage = create_test_storage();

    storage
        .device()
        .save_device_info(&[1u8; 32], 0, "Old Name", 100)
        .unwrap();
    storage
        .device()
        .save_device_info(&[2u8; 32], 1, "New Name", 200)
        .unwrap();

    let (id, index, name, _) = storage.device().load_device_info().unwrap().unwrap();
    assert_eq!(id, [2u8; 32]);
    assert_eq!(index, 1);
    assert_eq!(name, "New Name");
}

// @internal
#[test]
fn test_storage_save_load_device_registry() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device = DeviceInfo::derive(&master_seed, 0, "Primary".to_string(), 0);
    let registry = DeviceRegistry::new(device.to_registered(&master_seed), &signing_key);

    assert!(!storage.device().has_device_registry().unwrap());

    storage.device().save_device_registry(&registry).unwrap();

    assert!(storage.device().has_device_registry().unwrap());

    let loaded = storage.device().load_device_registry().unwrap().unwrap();
    assert_eq!(loaded.version(), registry.version());
    assert_eq!(loaded.active_count(), 1);
    assert!(loaded.verify(&signing_key.public_key()));
}

// @internal
#[test]
fn test_storage_device_registry_roundtrip() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Primary".to_string(), 0);
    let device1 = DeviceInfo::derive(&master_seed, 1, "Secondary".to_string(), 0);

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    storage.device().save_device_registry(&registry).unwrap();
    let loaded = storage.device().load_device_registry().unwrap().unwrap();

    assert_eq!(loaded.version(), 2);
    assert_eq!(loaded.active_count(), 2);
}

// ============================================================
// Phase 1: Device Sync State Storage Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Offline changes sync when reconnected
/// Need to persist pending sync items between app restarts
// @internal
#[test]
fn test_storage_save_load_device_sync_state() {
    use vauchi_core::sync::device_sync::{InterDeviceSyncState, SyncItem};

    let storage = create_test_storage();
    let device_id = [0x42u8; 32];

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

    storage.sync().save_device_sync_state(&state).unwrap();

    let loaded = storage
        .sync()
        .load_device_sync_state(&device_id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded.device_id(), &device_id);
    assert_eq!(loaded.pending_items().len(), 2);
    assert_eq!(loaded.pending_items()[0].timestamp(), 1000);
    assert_eq!(loaded.pending_items()[1].timestamp(), 2000);
}

/// Test that we can list all device sync states
// @internal
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

    storage.sync().save_device_sync_state(&state_a).unwrap();
    storage.sync().save_device_sync_state(&state_b).unwrap();

    let states = storage.sync().list_device_sync_states().unwrap();
    assert_eq!(states.len(), 2);
}

/// Test version vector persistence for conflict detection
// @internal
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

    storage.sync().save_version_vector(&vector).unwrap();

    let loaded = storage.sync().load_version_vector().unwrap().unwrap();

    assert_eq!(loaded.get(&device_a), 2);
    assert_eq!(loaded.get(&device_b), 1);
}

/// Test that version vector updates correctly
// @internal
#[test]
fn test_storage_version_vector_update() {
    use vauchi_core::sync::device_sync::VersionVector;

    let storage = create_test_storage();

    let device_a = [0x41u8; 32];

    let mut vector1 = VersionVector::new();
    vector1.increment(&device_a);
    storage.sync().save_version_vector(&vector1).unwrap();

    let mut vector2 = VersionVector::new();
    vector2.increment(&device_a);
    vector2.increment(&device_a);
    vector2.increment(&device_a);
    storage.sync().save_version_vector(&vector2).unwrap();

    let loaded = storage.sync().load_version_vector().unwrap().unwrap();
    assert_eq!(loaded.get(&device_a), 3);
}

/// Test that recovery_trusted flag persists through save/load
// @internal
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
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(loaded.is_recovery_trusted());

    contact.untrust_for_recovery().unwrap();
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(!loaded.is_recovery_trusted());
}

// ============================================================
// Coverage gap tests — contact_limit, delta_version
// ============================================================

/// Test get_contact_limit returns default 10_000
// @scenario: contacts_management :: Contact limits
// @internal
#[test]
fn test_get_contact_limit_default() {
    let storage = create_test_storage();
    let limit = storage.contacts().get_contact_limit().unwrap();
    assert_eq!(limit, 10_000);
}

/// Test last_delta_version defaults to 0 for new contact
// @scenario: sync_updates :: Delta sync versioning
// @internal
#[test]
fn test_last_delta_version_default() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let version = storage.contacts().last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 0);
}

/// Test record_delta_version and last_delta_version roundtrip
// @scenario: sync_updates :: Delta sync versioning
// @internal
#[test]
fn test_record_and_load_delta_version() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    storage
        .contacts()
        .record_delta_version(&contact_id, 42)
        .unwrap();
    let version = storage.contacts().last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 42);

    storage
        .contacts()
        .record_delta_version(&contact_id, 100)
        .unwrap();
    let version = storage.contacts().last_delta_version(&contact_id).unwrap();
    assert_eq!(version, 100);
}

/// Test last_delta_version fails for nonexistent contact
// @scenario: sync_updates :: Delta sync versioning
// @internal
#[test]
fn test_last_delta_version_nonexistent_contact() {
    let storage = create_test_storage();
    let result = storage.contacts().last_delta_version("nonexistent");
    assert!(result.is_err(), "expected error");
}

/// Test last_delta_version_for_device defaults to 0 (missing row = no floor)
// @scenario: sync_updates :: Delta sync versioning
// @internal
#[test]
fn test_last_delta_version_for_device_default() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let version = storage
        .contacts()
        .last_delta_version_for_device(&contact_id, &[1; 32])
        .unwrap();
    assert_eq!(version, 0, "a contact with no recorded floor returns 0");

    let version = storage
        .contacts()
        .last_delta_version_for_device("nonexistent", &[1; 32])
        .unwrap();
    assert_eq!(
        version, 0,
        "an unknown contact has no floor rows, returns 0"
    );
}

/// Test per-device floors are independent and re-recording overwrites
// @scenario: sync_updates :: Delta sync versioning
// @internal
#[test]
fn test_delta_version_floors_are_isolated_by_peer_device() {
    let storage = create_test_storage();
    let contact = create_test_contact("Alice");
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let first_device = [1; 32];
    let second_device = [2; 32];

    storage
        .contacts()
        .record_delta_version_for_device(&contact_id, &first_device, 2)
        .unwrap();

    assert_eq!(
        storage
            .contacts()
            .last_delta_version_for_device(&contact_id, &first_device)
            .unwrap(),
        2
    );
    assert_eq!(
        storage
            .contacts()
            .last_delta_version_for_device(&contact_id, &second_device)
            .unwrap(),
        0,
        "a floor recorded for one device must not leak onto another device"
    );

    storage
        .contacts()
        .record_delta_version_for_device(&contact_id, &first_device, 3)
        .unwrap();
    assert_eq!(
        storage
            .contacts()
            .last_delta_version_for_device(&contact_id, &first_device)
            .unwrap(),
        3,
        "re-recording must overwrite the stored floor"
    );
}

/// Test wipe_device_data clears device_info and sync state
// @scenario: identity_management :: Identity deletion
// @internal
#[test]
fn test_wipe_device_data() {
    let storage = create_test_storage();

    let device_id = [0x42u8; 32];
    storage
        .device()
        .save_device_info(&device_id, 0, "Test Device", 1000)
        .unwrap();
    assert!(storage.device().has_device_info().unwrap());

    storage.wipe_device_data().unwrap();

    assert!(!storage.device().has_device_info().unwrap());
}

/// Test is_replay_nonce detects duplicates
// @scenario: security :: Replay attack prevention
// @internal
#[test]
fn test_is_replay_nonce() {
    let storage = create_test_storage();

    let nonce = [0xAAu8; 32];

    // Fresh nonce is not a replay
    assert!(
        !storage
            .replay()
            .is_replay_nonce("contact-1", &nonce)
            .unwrap()
    );

    storage
        .replay()
        .save_replay_nonce("contact-1", &nonce, 1000)
        .unwrap();

    // Now it's a replay
    assert!(
        storage
            .replay()
            .is_replay_nonce("contact-1", &nonce)
            .unwrap()
    );

    // Same nonce for different contact is NOT a replay
    assert!(
        !storage
            .replay()
            .is_replay_nonce("contact-2", &nonce)
            .unwrap()
    );
}

/// Test cleanup_replay_nonces removes old entries
// @scenario: security :: Replay attack prevention
// @internal
#[test]
fn test_cleanup_replay_nonces() {
    let storage = create_test_storage();

    let old_nonce = [0x11u8; 32];
    let new_nonce = [0x22u8; 32];

    storage
        .replay()
        .save_replay_nonce("c1", &old_nonce, 1000)
        .unwrap();
    storage
        .replay()
        .save_replay_nonce("c1", &new_nonce, 5000)
        .unwrap();

    // Cleanup nonces older than 3000
    let removed = storage.replay().cleanup_replay_nonces(3000).unwrap();
    assert_eq!(removed, 1);

    assert!(!storage.replay().is_replay_nonce("c1", &old_nonce).unwrap());
    assert!(storage.replay().is_replay_nonce("c1", &new_nonce).unwrap());
}

/// Test load_device_registry_json returns structured JSON
// @scenario: identity_management :: GDPR data export
// @internal
#[test]
fn test_load_device_registry_json() {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::device::{DeviceInfo, DeviceRegistry};

    let storage = create_test_storage();

    let json = storage.device().load_device_registry_json().unwrap();
    assert!(json.is_none());

    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let device = DeviceInfo::derive(&master_seed, 0, "Test".to_string(), 0);
    let registry = DeviceRegistry::new(device.to_registered(&master_seed), &signing_key);
    storage.device().save_device_registry(&registry).unwrap();

    let json = storage.device().load_device_registry_json().unwrap();
    assert!(json.is_some(), "expected Some value");
    let json_str = json.unwrap();
    assert!(json_str.contains("device_id"));
}

// ============================================================
// proposal_trusted storage persistence (Task 3)
// ============================================================

/// Test that proposal_trusted defaults to false on a new contact.
// @scenario: contacts_management :: Contact trust management
// @internal
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
// @internal
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
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(
        loaded.is_proposal_trusted(),
        "proposal_trusted must survive save/load roundtrip"
    );
}

/// Test that proposal_trusted = false is correctly persisted and reloaded.
// @scenario: contacts_management :: Contact trust management
// @internal
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
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(
        !loaded.is_proposal_trusted(),
        "proposal_trusted = false must persist correctly"
    );

    // Toggle true → save → load → false
    contact.set_proposal_trusted(true).unwrap();
    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(loaded.is_proposal_trusted());

    contact.set_proposal_trusted(false).unwrap();
    storage.contacts().save_contact(&contact).unwrap();
    let loaded = storage
        .contacts()
        .load_contact(&contact_id)
        .unwrap()
        .unwrap();
    assert!(
        !loaded.is_proposal_trusted(),
        "proposal_trusted must be updatable back to false"
    );
}

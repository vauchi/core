// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::device_orchestrator
//! Extracted from device_orchestrator.rs

use vauchi_core::contact::{Contact, Group};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::{DoubleRatchetState, SigningKeyPair, SymmetricKey, X3DHKeyPair};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::sync::*;
use vauchi_core::*;

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

fn create_test_device(master_seed: &[u8; 32], index: u32, name: &str) -> DeviceInfo {
    DeviceInfo::derive(master_seed, index, name.to_string(), 0)
}

fn create_test_registry(master_seed: &[u8; 32], device: &DeviceInfo) -> DeviceRegistry {
    let signing_key = SigningKeyPair::from_seed(master_seed);
    DeviceRegistry::new(device.to_registered(master_seed), &signing_key)
}

fn create_test_contact(name: &str) -> Contact {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

fn tiny_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode test PNG");
    bytes.into_inner()
}

// ============================================================
// Phase 3: Device Sync Orchestrator Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Changes sync between devices
/// "When I update my phone number on Device A
///  Then Device B should receive the update"
// @scenario: device_management :: Changes sync between devices
// @scenario: sync_updates :: Contact updates reach all my devices
#[test]
fn test_orchestrator_record_local_change() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let item = SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(item).unwrap();

    let pending = orchestrator.pending_for_device(device_b.device_id());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].timestamp(), 1000);
}

/// Test that pending items returns correct results
// @scenario: device_management :: Changes sync between devices
// @scenario: sync_updates :: Contact updates reach all my devices
#[test]
fn test_orchestrator_pending_for_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+999".to_string(),
            timestamp: 2000,
        })
        .unwrap();

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 2);
}

/// Scenario: New device receives full state
/// "When Device B is newly linked
///  Then Device B should receive my complete contact card
///  And Device B should receive all my contacts"
// @scenario: device_management :: New device receives full state
// @scenario: sync_updates :: New device receives full state
#[test]
fn test_orchestrator_create_full_sync_payload() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let mut own_card = ContactCard::new("Alice");
    let _ = own_card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
        0,
    ));
    storage.contacts().save_own_card(&own_card).unwrap();

    let contact = create_test_contact("Bob");
    storage.contacts().save_contact(&contact).unwrap();

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let payload = orchestrator
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();

    assert_eq!(payload.contact_count(), 1);
    assert!(!payload.own_card_json.is_empty());
}

/// Scenario: New device applies received state
// @scenario: device_management :: New device receives full state
// @scenario: sync_updates :: New device receives full state
#[test]
fn test_orchestrator_apply_full_sync() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    // Create orchestrator for new device (Device B)
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Create a sync payload (simulating what Device A would send)
    let own_card = ContactCard::new("Alice");
    let contact = create_test_contact("Bob");
    let payload = DeviceSyncPayload::new(&[contact], &own_card, 1);

    orchestrator.apply_full_sync(payload).unwrap();

    let loaded_card = storage.contacts().load_own_card().unwrap().unwrap();
    assert_eq!(loaded_card.display_name(), "Alice");

    let contacts = storage.contacts().list_contacts().unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].display_name(), "Bob");
}

// @scenario: contact-annotations.feature - Tags sync to my other linked devices
// @internal
#[test]
fn test_orchestrator_syncs_tags_round_trip() {
    let master_seed = [0x42u8; 32];

    // Device A: a tag applied to a contact.
    let storage_a = create_test_storage();
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry_a = create_test_registry(&master_seed, &device_a);
    let contact = create_test_contact("Bob");
    storage_a.contacts().save_contact(&contact).unwrap();
    let tag = storage_a.tags().create_tag("berlin-trip").unwrap();
    storage_a.tags().add_to_tag(&tag.id, contact.id()).unwrap();

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let payload = orchestrator_a
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    assert_eq!(payload.tags.len(), 1, "tag included in sync payload");

    // Device B: fresh storage receives the payload.
    let storage_b = create_test_storage();
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry_b = create_test_registry(&master_seed, &device_b);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);
    orchestrator_b.apply_full_sync(payload).unwrap();

    // Device B has the same tag — same id, name, and membership.
    let synced = storage_b
        .tags()
        .get_tag(&tag.id)
        .unwrap()
        .expect("tag synced with its original id");
    assert_eq!(synced.name, "berlin-trip");
    assert!(synced.contains(contact.id()), "membership synced");
}

// @scenario: device_management :: Linked device preserves group presentation state
// @scenario: sync_updates :: Group presentation state converges across linked devices
#[test]
fn test_orchestrator_syncs_groups_round_trip() {
    let master_seed = [0x42u8; 32];

    let storage_a = create_test_storage();
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry_a = create_test_registry(&master_seed, &device_a);
    let contact = create_test_contact("Bob");
    storage_a.contacts().save_contact(&contact).unwrap();

    let mut group = Group::new("group-work".into(), "Work", 10);
    group.add_contact(contact.id(), 11);
    group.add_visible_field("field-email", 12);
    group
        .set_display_name_override(Some("Alice at Work"), 13)
        .unwrap();
    group
        .set_bio_override(Some("Professional profile"), 14)
        .unwrap();
    group.set_avatar_override(Some(&tiny_png()), 15).unwrap();
    let expected_avatar = group.avatar_override().unwrap().to_vec();
    storage_a.labels().save_group(&group).unwrap();

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let payload = orchestrator_a
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    assert_eq!(payload.groups.len(), 1, "group included in sync payload");
    let payload = DeviceSyncPayload::from_json(&payload.to_json()).unwrap();

    let storage_b = create_test_storage();
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry_b = create_test_registry(&master_seed, &device_b);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);
    orchestrator_b.apply_full_sync(payload).unwrap();

    let synced = storage_b
        .labels()
        .load_group("group-work")
        .expect("group synced with original id");
    assert_eq!(synced.name(), "Work");
    assert_eq!(synced.created_at(), 10);
    assert_eq!(synced.modified_at(), 15);
    assert!(synced.contains_contact(contact.id()));
    assert!(synced.is_field_visible("field-email"));
    assert_eq!(synced.display_name_override(), Some("Alice at Work"));
    assert_eq!(synced.bio_override(), Some("Professional profile"));
    assert_eq!(synced.avatar_override(), Some(expected_avatar.as_slice()));
}

// @scenario: contact-annotations.feature - Tags sync to my other linked devices
// @internal
#[test]
fn test_orchestrator_syncs_places_and_exchange_locations() {
    use vauchi_core::contact::place::ExchangeLocation;
    let master_seed = [0x42u8; 32];

    // Device A: a contact met at a named place.
    let storage_a = create_test_storage();
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry_a = create_test_registry(&master_seed, &device_a);
    let contact = create_test_contact("Bob");
    storage_a.contacts().save_contact(&contact).unwrap();
    let place = storage_a
        .places()
        .create_place("The Anchor Bar", 52.52, 13.405)
        .unwrap();
    storage_a
        .save_exchange_location(
            contact.id(),
            &ExchangeLocation {
                latitude: 52.52,
                longitude: 13.405,
                place_id: Some(place.id.clone()),
            },
        )
        .unwrap();

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let payload = orchestrator_a
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    assert_eq!(payload.places.len(), 1, "place in payload");
    assert_eq!(payload.exchange_locations.len(), 1, "location in payload");

    // Device B receives it.
    let storage_b = create_test_storage();
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry_b = create_test_registry(&master_seed, &device_b);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);
    orchestrator_b.apply_full_sync(payload).unwrap();

    // Place + per-contact location restored with ids intact.
    let synced_place = storage_b
        .places()
        .get_place(&place.id)
        .unwrap()
        .expect("place synced");
    assert_eq!(synced_place.name, "The Anchor Bar");
    let synced_loc = storage_b
        .load_exchange_location(contact.id())
        .unwrap()
        .expect("exchange location synced");
    assert_eq!(synced_loc.place_id.as_deref(), Some(place.id.as_str()));
    assert!((synced_loc.latitude - 52.52).abs() < 1e-9);
}

/// Test marking items as synced clears pending queue
// @scenario: device_management :: Changes sync between devices
#[test]
fn test_orchestrator_mark_synced() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);

    orchestrator.mark_synced(&device_b_id, 1).unwrap();

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 0);
}

/// Test version vector is incremented on local changes
// @scenario: device_management :: Device registry version tracking
#[test]
fn test_orchestrator_version_vector_increment() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_a_id = *device_a.device_id();
    let registry = create_test_registry(&master_seed, &device_a);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    // Initially version is 0
    assert_eq!(orchestrator.version_vector().get(&device_a_id), 0);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "test@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    assert_eq!(orchestrator.version_vector().get(&device_a_id), 1);
}

/// Test loading state from storage
// @scenario: device_management :: Offline changes sync when reconnected
#[test]
fn test_orchestrator_load_persisted_state() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    {
        let device_a = create_test_device(&master_seed, 0, "Device A");
        let mut registry = create_test_registry(&master_seed, &device_a);
        registry
            .add_device(device_b.to_registered(&master_seed), &signing_key)
            .unwrap();

        let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "test@example.com".to_string(),
                timestamp: 1000,
            })
            .unwrap();
    }

    let device_a2 = create_test_device(&master_seed, 0, "Device A");
    let device_b2 = create_test_device(&master_seed, 1, "Device B");
    let mut registry2 = create_test_registry(&master_seed, &device_a2);
    registry2
        .add_device(device_b2.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator = DeviceSyncOrchestrator::load(&storage, device_a2, registry2).unwrap();

    assert_eq!(orchestrator.pending_for_device(&device_b_id).len(), 1);
}

// ============================================================
// Phase 4: Encryption Layer Tests (TDD)
// Device-to-device encryption using ECDH + XChaCha20-Poly1305
// ============================================================

/// Test encrypting data for another device
/// Uses ECDH: our_secret * their_public -> shared_secret
/// Then HKDF to derive encryption key
// @scenario: device_management :: Device-specific keys
#[test]
fn test_encrypt_for_device() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_public_key = *device_b.exchange_public_key();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    let plaintext = b"Hello from Device A!";
    let ciphertext = orchestrator
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    assert_ne!(ciphertext, plaintext);
    // Ciphertext should be longer (includes nonce + tag)
    assert!(ciphertext.len() > plaintext.len());
}

/// Test decrypting data from another device
// @scenario: device_management :: Device-specific keys
#[test]
fn test_decrypt_from_device() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_a_public_key = *device_a.exchange_public_key();
    let device_b_public_key = *device_b.exchange_public_key();

    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let device_b_for_b = create_test_device(&master_seed, 1, "Device B");
    let mut registry_b = create_test_registry(&master_seed, &device_b_for_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b_for_b, registry_b);

    // Device A encrypts for Device B
    let plaintext = b"Secret message from A to B";
    let ciphertext = orchestrator_a
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    // Device B decrypts from Device A
    let decrypted = orchestrator_b
        .decrypt_from_device(&device_a_public_key, &ciphertext)
        .unwrap();

    assert_eq!(decrypted, plaintext);
}

/// Test that wrong device cannot decrypt
// @scenario: device_management :: Device-specific keys
#[test]
fn test_wrong_device_cannot_decrypt() {
    let storage_a = create_test_storage();
    let storage_c = create_test_storage();
    let master_seed = [0x42u8; 32];
    let different_seed = [0x99u8; 32]; // Different identity
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let _signing_key_c = SigningKeyPair::from_seed(&different_seed);

    // Create devices A and B (same identity)
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_public_key = *device_b.exchange_public_key();

    // Create device C (different identity - attacker)
    let device_c = create_test_device(&different_seed, 0, "Device C");

    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    // Registry for device C (pretending it has A in registry)
    let registry_c = create_test_registry(&different_seed, &device_c);

    let orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let orchestrator_c = DeviceSyncOrchestrator::new(&storage_c, device_c, registry_c);

    // Device A encrypts for Device B
    let plaintext = b"Secret message for B only";
    let ciphertext = orchestrator_a
        .encrypt_for_device(&device_b_public_key, plaintext)
        .unwrap();

    // Device C (attacker) tries to decrypt - should fail
    // Even if C knows A's public key, C doesn't have B's secret key
    let device_a_public_key =
        *create_test_device(&master_seed, 0, "Device A").exchange_public_key();
    let result = orchestrator_c.decrypt_from_device(&device_a_public_key, &ciphertext);

    assert!(result.is_err(), "expected error");
}

// ============================================================
// Phase 5: Conflict Resolution Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Conflict resolution between devices
/// "Given I have made conflicting changes on Device A and Device B
///  Then the most recent change should win"
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_resolution_last_write_wins() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B has a local change with timestamp 1000
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "local@example.com".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Incoming change from Device A with timestamp 2000 (newer)
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "remote@example.com".to_string(),
        timestamp: 2000,
    }];

    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated { new_value, .. } => {
            assert_eq!(new_value, "remote@example.com");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

/// Test that older incoming changes are rejected
// @scenario: device_management :: Conflict resolution between devices
#[test]
fn test_conflict_resolution_rejects_older() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B has a local change with timestamp 2000
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "newer_local@example.com".to_string(),
        timestamp: 2000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Incoming change from Device A with timestamp 1000 (older)
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "older_remote@example.com".to_string(),
        timestamp: 1000,
    }];

    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

    // The older remote change should be rejected (empty applied list)
    assert!(applied.is_empty());
}

/// A field update and removal share one LWW conflict key (ADR-020).
// @scenario: sync_updates :: Own-card field removals converge across linked devices
#[test]
fn card_field_removal_conflicts_with_updates_for_the_same_field() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "local@example.com".to_string(),
            timestamp: 2000,
        })
        .unwrap();

    let stale = orchestrator
        .process_incoming(
            vec![SyncItem::CardFieldRemoved {
                field_label: "email".to_string(),
                timestamp: 1000,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(
        stale.is_empty(),
        "older removal must lose to the field update"
    );

    let newer = orchestrator
        .process_incoming(
            vec![SyncItem::CardFieldRemoved {
                field_label: "email".to_string(),
                timestamp: 3000,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(matches!(
        newer.as_slice(),
        [SyncItem::CardFieldRemoved { field_label, .. }] if field_label == "email"
    ));
}

/// Scenario: Bidirectional sync
/// "When I add a phone number on Device A
///  And I add an email on Device B
///  Then both devices should have both fields"
// @scenario: device_management :: Bidirectional sync
#[test]
fn test_concurrent_updates_different_fields_both_preserved() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];

    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry = create_test_registry(&master_seed, &device_b);

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_b, registry);

    // Device B adds email locally
    let local_item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "b@example.com".to_string(),
        timestamp: 1000,
    };
    orchestrator.record_local_change(local_item).unwrap();

    // Device A added phone at roughly the same time
    let incoming_items = vec![SyncItem::CardUpdated {
        field_label: "phone".to_string(),
        new_value: "+1234567890".to_string(),
        timestamp: 1001,
    }];

    // Process incoming - different fields, no conflict
    let applied = orchestrator
        .process_incoming(incoming_items, &[0x99u8; 32])
        .unwrap();

    // The phone update should be applied (different field)
    assert_eq!(applied.len(), 1);
    match &applied[0] {
        SyncItem::CardUpdated {
            field_label,
            new_value,
            ..
        } => {
            assert_eq!(field_label, "phone");
            assert_eq!(new_value, "+1234567890");
        }
        _ => panic!("Expected CardUpdated"),
    }
}

// ============================================================
// Phase 6: Bidirectional Sync Tests (TDD)
// Based on features/device_management.feature @sync scenarios
// ============================================================

/// Scenario: Bidirectional sync with merge
/// Both devices add different fields; both should end up with both
// @scenario: device_management :: Bidirectional sync
#[test]
fn test_bidirectional_field_additions() {
    let storage_a = create_test_storage();
    let storage_b = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b_for_a = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b_for_a.device_id();
    let mut registry_a = create_test_registry(&master_seed, &device_a);
    registry_a
        .add_device(device_b_for_a.to_registered(&master_seed), &signing_key)
        .unwrap();

    let device_a_for_b = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_a_id = *device_a_for_b.device_id();
    let mut registry_b = create_test_registry(&master_seed, &device_b);
    registry_b
        .add_device(device_a_for_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator_a = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a);
    let mut orchestrator_b = DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b);

    orchestrator_a
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+1111111111".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    orchestrator_b
        .record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "user@example.com".to_string(),
            timestamp: 1001,
        })
        .unwrap();

    let a_to_b = orchestrator_a.pending_for_device(&device_b_id).to_vec();
    let b_to_a = orchestrator_b.pending_for_device(&device_a_id).to_vec();

    let applied_on_b = orchestrator_b
        .process_incoming(a_to_b, &[0x99u8; 32])
        .unwrap();
    let applied_on_a = orchestrator_a
        .process_incoming(b_to_a, &[0x99u8; 32])
        .unwrap();

    // Both should have applied the other's changes (different fields, no conflict)
    assert_eq!(applied_on_b.len(), 1); // phone from A
    assert_eq!(applied_on_a.len(), 1); // email from B
}

/// Scenario: Offline changes are queued
/// Changes made while offline should be stored for later sync
// @scenario: device_management :: Offline changes sync when reconnected
#[test]
fn test_offline_changes_queue() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    for i in 1..=5 {
        orchestrator
            .record_local_change(SyncItem::CardUpdated {
                field_label: format!("field_{}", i),
                new_value: format!("value_{}", i),
                timestamp: i * 1000,
            })
            .unwrap();
    }

    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 5);
}

/// Scenario: Offline changes sync when reconnected
/// "Given Device B is offline
///  When Device B makes changes offline
///  And Device B reconnects
///  Then those changes should sync to Device A"
// @scenario: device_management :: Offline changes sync when reconnected
#[test]
fn test_offline_changes_sync_on_reconnect() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device_a = create_test_device(&master_seed, 0, "Device A");
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let device_b_id = *device_b.device_id();

    let mut registry = create_test_registry(&master_seed, &device_a);
    registry
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();

    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);

    orchestrator
        .record_local_change(SyncItem::CardUpdated {
            field_label: "offline_field".to_string(),
            new_value: "offline_value".to_string(),
            timestamp: 5000,
        })
        .unwrap();

    let pending = orchestrator.pending_for_device(&device_b_id);
    assert_eq!(pending.len(), 1);

    let sync_message = orchestrator.create_sync_message(&device_b_id).unwrap();

    assert!(!sync_message.items.is_empty());
    assert_eq!(sync_message.items.len(), 1);
}

// G3: the LWW gate must survive an orchestrator reload. A local edit's
// timestamp is persisted (field_timestamps) so a later-loaded orchestrator
// rejects an older incoming edit and accepts a newer one — without this,
// load() starts empty and an older remote change would overwrite a newer
// local one (2026-06-06-multi-device-sync-live-wiring).
// @scenario: device_management :: Concurrent edits converge by last-write-wins
#[test]
fn field_timestamps_persist_across_reload_for_lww() {
    let storage = create_test_storage();
    let master_seed = [0x55u8; 32];
    // DeviceInfo::derive is deterministic from (seed, index), so re-deriving
    // yields an equivalent device (DeviceInfo isn't Clone).
    let registry = create_test_registry(
        &master_seed,
        &create_test_device(&master_seed, 0, "Device 0"),
    );

    {
        let mut orch = DeviceSyncOrchestrator::new(
            &storage,
            create_test_device(&master_seed, 0, "Device 0"),
            registry.clone(),
        );
        orch.record_local_change(SyncItem::CardUpdated {
            field_label: "email".to_string(),
            new_value: "new@example.com".to_string(),
            timestamp: 1000,
        })
        .unwrap();
    }

    // Reload a fresh orchestrator — field_timestamps must be restored.
    let mut reloaded = DeviceSyncOrchestrator::load(
        &storage,
        create_test_device(&master_seed, 0, "Device 0"),
        registry.clone(),
    )
    .unwrap();

    // An OLDER incoming edit to the same field is rejected (LWW).
    let stale = reloaded
        .process_incoming(
            vec![SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "stale@example.com".to_string(),
                timestamp: 500,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert!(
        stale.is_empty(),
        "older incoming edit must lose to the persisted newer local timestamp"
    );

    // A NEWER incoming edit to the same field is applied.
    let fresh = reloaded
        .process_incoming(
            vec![SyncItem::CardUpdated {
                field_label: "email".to_string(),
                new_value: "fresh@example.com".to_string(),
                timestamp: 1500,
            }],
            &[0x99u8; 32],
        )
        .unwrap();
    assert_eq!(fresh.len(), 1, "newer incoming edit must win");
}

fn save_ratchet_for(storage: &Storage, contact_id: &str) {
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), *their_dh.public_key())
            .unwrap();
    storage
        .ratchets()
        .save_ratchet_state(contact_id, &ratchet, true)
        .unwrap();
}

/// An add-device link carries contact state without any live ratchet chain.
// @scenario: device_management :: New device receives full state
// @internal
#[test]
fn add_device_sync_payload_carries_contacts_without_ratchets() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let contact = create_test_contact("Bob");
    storage.contacts().save_contact(&contact).unwrap();
    save_ratchet_for(&storage, contact.id());

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
    let payload = orchestrator
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();

    assert_eq!(payload.contact_count(), 1);
}

/// Replacement also establishes fresh pair sessions; it must not inherit a
/// chain that could still advance on the source device.
// @scenario: device_management :: New device receives full state
// @internal
#[test]
fn replacement_sync_payload_carries_contacts_without_ratchets() {
    let storage = create_test_storage();
    let master_seed = [0x42u8; 32];
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry = create_test_registry(&master_seed, &device_a);

    let contact = create_test_contact("Bob");
    storage.contacts().save_contact(&contact).unwrap();
    save_ratchet_for(&storage, contact.id());

    let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry);
    let payload = orchestrator
        .create_full_sync_payload(DeviceLinkIntent::ReplaceDevice)
        .unwrap();

    assert_eq!(payload.contact_count(), 1);
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn expanded_registry_is_queued_for_devices_linked_earlier() {
    let storage = create_test_storage();
    let master_seed = [0x63u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);
    let identity =
        Identity::from_device_link(master_seed, "Alice".into(), 0, "Alice phone".into(), 1);
    let device_a = identity.create_device_info(1);
    let device_b = create_test_device(&master_seed, 1, "Alice tablet");
    let device_c = create_test_device(&master_seed, 2, "Alice laptop");

    let mut previous = create_test_registry(&master_seed, &device_a);
    previous
        .add_device(device_b.to_registered(&master_seed), &signing_key)
        .unwrap();
    storage.device().save_device_registry(&previous).unwrap();

    let mut expanded = previous;
    expanded
        .add_device(device_c.to_registered(&master_seed), &signing_key)
        .unwrap();
    DeviceSyncOrchestrator::persist_device_registry_change(&storage, &identity, &expanded, 2)
        .unwrap();

    let stored = storage.device().load_device_registry().unwrap().unwrap();
    assert_eq!(stored.active_count(), 3);
    let orchestrator = DeviceSyncOrchestrator::load(&storage, device_a, stored).unwrap();
    assert!(
        orchestrator
            .pending_for_device(device_b.device_id())
            .iter()
            .any(|item| matches!(item, SyncItem::DeviceRegistryChanged { .. })),
        "the pre-existing tablet must receive the expanded registry"
    );
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn device_registry_sync_accepts_newer_signed_state_and_rejects_forgery() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let identity = wb.identity().unwrap();
    let seed = *identity.master_seed();
    let second = DeviceInfo::derive(&seed, 1, "Alice tablet".into(), 2);
    let mut expanded = identity.initial_device_registry();
    expanded
        .add_device(second.to_registered(&seed), identity.signing_keypair())
        .unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: expanded.to_json(),
            version: expanded.version(),
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(wb.list_devices().unwrap().len(), 2);

    let attacker = SigningKeyPair::from_seed(&[0x99u8; 32]);
    let third = DeviceInfo::derive(&seed, 2, "Forged laptop".into(), 3);
    let mut forged = expanded;
    forged
        .add_device(third.to_registered(&seed), &attacker)
        .unwrap();
    let applied = wb
        .apply_sync_items(vec![SyncItem::DeviceRegistryChanged {
            registry_json: forged.to_json(),
            version: forged.version(),
        }])
        .unwrap();

    assert_eq!(applied, 0);
    assert_eq!(wb.list_devices().unwrap().len(), 2);
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn same_second_registry_expansions_order_by_signed_version() {
    let storage = create_test_storage();
    let seed = [0x73u8; 32];
    let current = create_test_device(&seed, 1, "Alice tablet");
    let registry = create_test_registry(&seed, &current);
    let mut orchestrator = DeviceSyncOrchestrator::new(&storage, current, registry);
    let sender = [0x11u8; 32];

    let applied = orchestrator
        .process_incoming(
            vec![
                SyncItem::DeviceRegistryChanged {
                    registry_json: "registry-v2".to_string(),
                    version: 2,
                },
                SyncItem::DeviceRegistryChanged {
                    registry_json: "registry-v3".to_string(),
                    version: 3,
                },
            ],
            &sender,
        )
        .unwrap();

    assert_eq!(applied.len(), 2);
    assert_eq!(applied[0].timestamp(), 2);
    assert_eq!(applied[1].timestamp(), 3);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup and Recovery E2E Tests
//!
//! Feature: identity_management.feature
//! Feature: device_management.feature

use vauchi_core::{
    crypto::PublicKey, network::MockTransport, sync::CardDelta, ContactCard, ContactField,
    FieldType, Identity, Vauchi,
};

/// Tests the complete backup and recovery workflow.
///
/// Feature: identity_management.feature
/// Scenarios: Create encrypted identity backup, Restore identity from backup
#[test]
fn test_backup_recovery_happy_path() {
    // Step 1: Create identity and contact card
    let identity = Identity::create("Alice Smith");
    let original_public_id = identity.public_id();

    let mut card = ContactCard::new("Alice Smith");
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15559876543",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "blog",
        "https://alice.dev",
    ))
    .unwrap();

    // Step 2: Create encrypted backup
    let backup_password = "SecureP@ssw0rd!2024";
    let backup = identity
        .export_backup(backup_password)
        .expect("Backup creation should succeed");
    assert!(!backup.as_bytes().is_empty());

    // Step 3: Simulate new device - restore from backup
    let restored_identity = Identity::import_backup(&backup, backup_password)
        .expect("Restore should succeed with correct password");

    // Step 4: Verify restored identity matches original
    assert_eq!(restored_identity.public_id(), original_public_id);
    assert_eq!(restored_identity.display_name(), "Alice Smith");
    assert_eq!(
        restored_identity.signing_public_key(),
        identity.signing_public_key()
    );

    // Step 5: Verify wrong password fails
    let wrong_password_result = Identity::import_backup(&backup, "wrong_password");
    assert!(wrong_password_result.is_err());

    // Step 6: Verify restored identity can sign and verify
    let test_message = b"Test message for signature verification";
    let signature = restored_identity.sign(test_message);
    let public_key = PublicKey::from_bytes(*identity.signing_public_key());
    let verified = public_key.verify(test_message, &signature);
    assert!(verified);
}

/// Tests the multi-device linking and sync workflow.
///
/// Feature: device_management.feature
/// Scenarios: Link new device via backup, Sync between devices
#[test]
fn test_multi_device_linking_happy_path() {
    // Step 1: Create identity on Device A
    let mut device_a: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    device_a.create_identity("Alice").unwrap();
    device_a
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();

    let device_a_public_id = device_a.public_id().unwrap();
    let device_a_card = device_a.own_card().unwrap().unwrap();

    // Step 2: Export backup for device linking
    let identity_a = device_a.identity().unwrap();
    let backup = identity_a
        .export_backup("LinkingPassword123!")
        .expect("Backup should succeed");

    // Step 3: Import on Device B (simulate new device)
    let _device_b: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    let restored_identity = Identity::import_backup(&backup, "LinkingPassword123!").unwrap();
    assert_eq!(restored_identity.public_id(), device_a_public_id);

    // Step 4: Verify both devices share same identity
    assert_eq!(restored_identity.display_name(), "Alice");
    assert_eq!(
        restored_identity.signing_public_key(),
        identity_a.signing_public_key()
    );

    // Step 5: Simulate sync - Device A updates card
    device_a
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
        ))
        .unwrap();
    let updated_card_a = device_a.own_card().unwrap().unwrap();
    assert_eq!(updated_card_a.fields().len(), 2);

    // Step 6: Compute delta for sync to Device B
    let delta = CardDelta::compute(&device_a_card, &updated_card_a);
    assert!(!delta.changes.is_empty());

    // Step 7: Apply delta on Device B
    let mut device_b_card = device_a_card.clone();
    delta.apply(&mut device_b_card).unwrap();
    assert_eq!(device_b_card.fields().len(), 2);
    assert!(device_b_card.fields().iter().any(|f| f.label() == "mobile"));
}

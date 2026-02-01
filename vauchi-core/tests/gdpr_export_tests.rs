// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR: Enhanced Export Tests
//!
//! Feature file: features/privacy_compliance.feature @export @enhanced
//! Tests for enhanced data export with devices, recovery, and consent.

mod common;

use vauchi_core::api::gdpr::{export_all_data, GdprExport};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn setup_storage_with_contacts() -> Storage {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Add a contact
    let mut card = ContactCard::new("Test Contact");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "test@example.com",
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([1u8; 32], card, shared_key);
    storage.save_contact(&contact).unwrap();

    // Save own card
    let mut own_card = ContactCard::new("My Name");
    own_card
        .add_field(ContactField::new(FieldType::Phone, "Mobile", "+1-555-0100"))
        .unwrap();
    storage.save_own_card(&own_card).unwrap();

    storage
}

#[test]
fn test_export_includes_contacts() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert_eq!(export.contacts.len(), 1, "Export should include contacts");
    assert_eq!(export.contacts[0].display_name, "Test Contact");
    assert!(!export.contacts[0].card_fields.is_empty());
}

#[test]
fn test_export_includes_own_card() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert!(export.own_card.is_some(), "Export should include own card");
}

#[test]
fn test_export_excludes_private_keys() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    let json = serde_json::to_string(&export).unwrap();
    // Should not contain "shared_key" or "private_key" in the export
    assert!(
        !json.contains("shared_key"),
        "Export should not contain shared keys"
    );
    assert!(
        !json.contains("private_key"),
        "Export should not contain private keys"
    );
}

#[test]
fn test_export_includes_consent_records() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Add consent records
    storage
        .execute_consent_upsert("consent-1", "data_processing", true, 1000)
        .unwrap();
    storage
        .execute_consent_upsert("consent-2", "contact_sharing", true, 1001)
        .unwrap();

    let export = export_all_data(&storage).unwrap();

    assert!(
        !export.settings.consent_records.is_empty(),
        "Export should include consent records"
    );
}

#[test]
fn test_export_includes_devices() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // devices field should be present (may be empty if no devices registered)
    assert!(
        export.devices.is_some(),
        "Export should include devices field"
    );
}

#[test]
fn test_export_includes_recovery_config() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // recovery_config field should be present
    assert!(
        export.recovery_config.is_some(),
        "Export should include recovery config field"
    );
}

#[test]
fn test_export_version_and_timestamp() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert!(export.version >= 1, "Export version should be at least 1");
    assert!(export.exported_at > 0, "Export timestamp should be set");
}
